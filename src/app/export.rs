//! 导出 PNG 文件、复制图片到剪贴板。
//!
//! 成品不常驻缓存：点复制、导出时才按需生成，一边算一边编码成 PNG。
//! 编码按 512 行横带往前推，不分配整张成品 RGBA。每条横带对和它相交的图
//! 各解码一次，缩放后只取这一带的行；缩放结果会留到这张图最后一行画完，
//! 所以峰值上界是「所有仍跨当前横带的缩放图面积之和」，不是整幅成品面积。
//! 源文件读不到、或者实际尺寸和项目里记录的不一致时整次导出失败，报出具体
//! 文件名，不静默少一张。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use gpui::{Context, Window};
use image::RgbImage;
use image::imageops::FilterType;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand};
use crate::imaging::layout::{self, Placement};
use crate::imaging::render;
use crate::model::{Mode, Rgba, SourceImage};
use crate::platform;

use super::AppRoot;
use super::actions::{CopyImage, ExportPng};

/// 导出时一次算多少行横带。512 行在 4K 画布上约 8MB RGBA，够小又不至于让
/// 每带重复解码太多次。
pub(crate) const EXPORT_BAND_ROWS: u32 = 512;

/// 导出时先写的临时文件路径：和目标文件同目录，后缀加 `.part`。
fn temp_sibling(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("export.png"));
    name.push(".part");
    path.with_file_name(name)
}

/// 一次导出里的一张图。只存来源和裁剪信息，像素到该用的横带才读。
struct ExportPart {
    name: String,
    source: SourceImage,
    band: CropBand,
    /// 项目里记录的原图尺寸，用来发现磁盘上的文件被换过。
    source_size: (u32, u32),
    /// 裁剪后在成品里的尺寸（原图像素）。
    cropped_size: (u32, u32),
}

/// 一次导出的全部内容。
struct ExportJob {
    /// 字幕还是排版，只影响成品文件名。
    mode: Mode,
    parts: Vec<ExportPart>,
    /// 每张图在成品画布里的位置。
    placements: Vec<Placement>,
    width: u32,
    height: u32,
    /// 缝隙底色，也是成品没有图片覆盖处的颜色。
    background: Rgba,
}

impl ExportJob {
    fn output_size(&self) -> (u32, u32) {
        (self.width.max(1), self.height.max(1))
    }

    fn file_name(&self, language: Language) -> String {
        let kind = match self.mode {
            Mode::Subtitle => language.export_kind_subtitle(),
            Mode::Layout => language.export_kind_layout(),
        };
        language.export_file_name(kind, &platform::timestamp())
    }

    /// 复制用：把整份 PNG 编码进内存（剪贴板接口要求完整的 PNG 字节）。
    fn encode_bytes(&self) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        self.encode_into(&mut bytes)?;
        Ok(bytes)
    }

    /// 导出用：直接把编码结果写进文件，不留整份 PNG 字节。
    ///
    /// 先写同目录的临时文件，成功后再改名：中途任何一张图读不到或尺寸变了，
    /// 用户选的目标路径不会留下半个 PNG，同名的旧文件也不会被提前清掉。
    fn encode_file(&self, path: &Path) -> Result<(), String> {
        let temp = temp_sibling(path);
        let result = (|| -> Result<(), String> {
            let file = File::create(&temp)
                .map_err(|err| format!("创建文件 {} 失败：{err}", temp.display()))?;
            let mut writer = BufWriter::new(file);
            self.encode_into(&mut writer)?;
            writer
                .flush()
                .map_err(|err| format!("写入文件 {} 失败：{err}", temp.display()))
        })();
        match result {
            Ok(()) => std::fs::rename(&temp, path).map_err(|err| {
                let _ = std::fs::remove_file(&temp);
                format!("保存文件 {} 失败：{err}", path.display())
            }),
            Err(err) => {
                let _ = std::fs::remove_file(&temp);
                Err(err)
            }
        }
    }

    /// 按 512 行横带流式编码成 PNG。
    fn encode_into<W: Write>(&self, sink: W) -> Result<(), String> {
        let (width, height) = self.output_size();
        let mut encoder = png::Encoder::new(sink, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| format!("编码 PNG 失败：{err}"))?;
        // 当前横带里用得到的已缩放图。某张图还有后续行时就留着，不再重复解码。
        let mut scaled: HashMap<usize, RgbImage> = HashMap::new();
        {
            let mut stream = writer
                .stream_writer()
                .map_err(|err| format!("编码 PNG 失败：{err}"))?;
            let mut y = 0u32;
            while y < height {
                let rows = EXPORT_BAND_ROWS.min(height - y);
                let band = self.render_band(y, rows, &mut scaled)?;
                stream
                    .write_all(&band)
                    .map_err(|err| format!("写出 PNG 失败：{err}"))?;
                // 已经画完、后面不会再出现的图从缓存里丢掉。
                let next = y + rows;
                scaled.retain(|index, _| {
                    self.placements
                        .get(*index)
                        .is_some_and(|placement| placement.y + placement.h > next)
                });
                y += rows;
            }
            stream
                .flush()
                .map_err(|err| format!("写出 PNG 失败：{err}"))?;
        }
        writer
            .finish()
            .map_err(|err| format!("写出 PNG 失败：{err}"))
    }

    /// 算一条横带：先铺底色，再把和这一带相交的图按行贴上去。
    fn render_band(
        &self,
        y: u32,
        rows: u32,
        scaled: &mut HashMap<usize, RgbImage>,
    ) -> Result<Vec<u8>, String> {
        let width = self.width.max(1) as usize;
        let mut buffer = vec![0u8; width * rows as usize * 4];
        let background = self.background;
        for pixel in buffer.chunks_exact_mut(4) {
            pixel[0] = background.r;
            pixel[1] = background.g;
            pixel[2] = background.b;
            pixel[3] = background.a;
        }
        for (index, part) in self.parts.iter().enumerate() {
            let Some(placement) = self.placements.get(index) else {
                continue;
            };
            let top = placement.y;
            let bottom = placement.y.saturating_add(placement.h);
            if bottom <= y || top >= y.saturating_add(rows) {
                continue;
            }
            if !scaled.contains_key(&index) {
                let prepared = self.prepare_part(part, placement)?;
                scaled.insert(index, prepared);
            }
            let image = scaled.get(&index).expect("刚刚放进缓存");
            if placement.x as usize + placement.w as usize > width {
                return Err(format!(
                    "图片 {} 在成品画布上越界：左边 {} + 宽 {} 超过画布宽 {}",
                    part.name, placement.x, placement.w, width
                ));
            }
            let draw_w = placement.w as usize;
            if draw_w == 0
                || placement.h == 0
                || image.width() as usize != draw_w
                || image.height() as usize != placement.h as usize
            {
                return Err(format!(
                    "图片 {} 的缩放结果尺寸不对：期望 {}×{}，实际 {}×{}",
                    part.name,
                    placement.w,
                    placement.h,
                    image.width(),
                    image.height()
                ));
            }
            let first_row = y.max(top);
            let last_row = (y.saturating_add(rows)).min(bottom);
            let copy_rows = last_row.saturating_sub(first_row) as usize;
            if copy_rows == 0 {
                continue;
            }
            let src_row0 = first_row.saturating_sub(top) as usize;
            let dst_row0 = first_row.saturating_sub(y) as usize;
            let src = image.as_raw();
            let src_stride = draw_w * 3;
            for row in 0..copy_rows {
                let src_start = (src_row0 + row) * src_stride;
                let dst_start = (dst_row0 + row) * width * 4 + placement.x as usize * 4;
                let src_row = &src[src_start..src_start + src_stride];
                let dst_row = &mut buffer[dst_start..dst_start + draw_w * 4];
                for (src_pixel, dst_pixel) in
                    src_row.chunks_exact(3).zip(dst_row.chunks_exact_mut(4))
                {
                    dst_pixel[0] = src_pixel[0];
                    dst_pixel[1] = src_pixel[1];
                    dst_pixel[2] = src_pixel[2];
                    dst_pixel[3] = 255;
                }
            }
        }
        Ok(buffer)
    }

    /// 读出一张图的源像素，裁剪并缩放到它在成品里的尺寸。解码出的整图只活
    /// 在这一趟里，返回时已经释放。
    fn prepare_part(&self, part: &ExportPart, placement: &Placement) -> Result<RgbImage, String> {
        let rgb = part
            .source
            .load_rgb()
            .map_err(|err| format!("读取图片 {} 失败：{err}", part.name))?;
        let actual = rgb.dimensions();
        if actual != part.source_size {
            return Err(format!(
                "图片 {} 的尺寸变了：记录 {}×{}，实际 {}×{}；请重新导入这张图",
                part.name, part.source_size.0, part.source_size.1, actual.0, actual.1
            ));
        }
        let cropped = render::crop_band_rgb(&rgb, part.band);
        let (width, height) = (placement.w.max(1), placement.h.max(1));
        Ok(image::imageops::resize(
            &cropped,
            width,
            height,
            FilterType::Lanczos3,
        ))
    }
}

impl AppRoot {
    /// 按当前项目拼一次导出任务。只收集来源和几何信息，不解码。
    fn build_export(&self) -> Option<ExportJob> {
        let inclusions = crop::resolve(&self.project);
        let gap = self.project.settings.gap;
        let gap_px = gap.effective_px();
        let mut parts: Vec<ExportPart> = Vec::new();
        for (index, item) in self.project.items.iter().enumerate() {
            let Some(inclusion) = inclusions.get(index).copied() else {
                continue;
            };
            let Some(cropped_size) = inclusion.output_size(item) else {
                continue;
            };
            parts.push(ExportPart {
                name: item.name.clone(),
                source: item.source.clone(),
                band: inclusion.band_for(item),
                source_size: (item.width.max(1), item.height.max(1)),
                cropped_size: (cropped_size.0.max(1), cropped_size.1.max(1)),
            });
        }
        if parts.is_empty() {
            return None;
        }
        let background = if gap.enabled { gap.color } else { Rgba::BLACK };
        match self.project.mode {
            Mode::Subtitle => {
                let width = parts
                    .iter()
                    .map(|part| part.cropped_size.0)
                    .max()
                    .unwrap_or(1)
                    .max(1);
                let mut placements = Vec::with_capacity(parts.len());
                let mut y = 0u32;
                for (index, part) in parts.iter().enumerate() {
                    let height = (part.cropped_size.1 as u64 * width as u64
                        / part.cropped_size.0.max(1) as u64)
                        .max(1) as u32;
                    placements.push(Placement {
                        item: index,
                        x: 0,
                        y,
                        w: width,
                        h: height,
                    });
                    y = y.saturating_add(height).saturating_add(gap_px);
                }
                let total_height = y.saturating_sub(gap_px).max(1);
                Some(ExportJob {
                    mode: Mode::Subtitle,
                    parts,
                    placements,
                    width,
                    height: total_height,
                    background,
                })
            }
            Mode::Layout => {
                let sizes: Vec<(u32, u32)> = parts.iter().map(|part| part.cropped_size).collect();
                let arranged = layout::arrange(&sizes, self.project.settings.layout, gap_px);
                Some(ExportJob {
                    mode: Mode::Layout,
                    parts,
                    placements: arranged.placements,
                    width: arranged.canvas_w.max(1),
                    height: arranged.canvas_h.max(1),
                    background,
                })
            }
        }
    }

    pub(crate) fn action_export(
        &mut self,
        _: &ExportPng,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_png(cx);
    }

    /// 导出 PNG 文件：先问保存路径，再在后台按横带流式写文件。
    pub(crate) fn export_png(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            self.set_status(self.language.export_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.export_no_images(), true, cx);
            return;
        };
        if self.export_in_flight {
            self.set_status(self.language.generating(), false, cx);
            return;
        }
        let file_name = job.file_name(self.language);
        let directory = platform::default_save_dir();
        let receiver = cx.prompt_for_new_path(&directory, Some(&file_name));
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                let _ = this.update(cx, |this, cx| {
                    if this.export_in_flight {
                        this.set_status(this.language.generating(), false, cx);
                        return;
                    }
                    this.export_in_flight = true;
                    this.export_generation = this.export_generation.wrapping_add(1);
                    let generation = this.export_generation;
                    let epoch = this.session_epoch;
                    this.set_status(this.language.generating(), false, cx);
                    let save_path = path.clone();
                    cx.spawn(async move |this, cx| {
                        let written = cx
                            .background_executor()
                            .spawn(async move { job.encode_file(&save_path) })
                            .await;
                        let display_path = path.display().to_string();
                        let _ = this.update(cx, |this, cx| {
                            if this.export_generation != generation {
                                return;
                            }
                            this.export_in_flight = false;
                            if this.session_epoch != epoch {
                                return;
                            }
                            match written {
                                Ok(()) => {
                                    this.set_status(this.language.saved(&display_path), false, cx);
                                    this.mark_success_flash("export-png");
                                    this.maybe_start_clear_countdown(cx);
                                }
                                Err(err) => {
                                    this.set_status(this.language.generate_failed(&err), true, cx)
                                }
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                });
            }
            Ok(Ok(None)) => {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(this.language.save_cancelled(), false, cx)
                });
            }
            Ok(Err(err)) => {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(this.language.save_failed(&err.to_string()), true, cx)
                });
            }
            Err(_) => {}
        })
        .detach();
    }

    pub(crate) fn action_copy(
        &mut self,
        _: &CopyImage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_image(cx);
    }

    /// 复制到剪贴板：点下去才按需生成，后台按横带编码，好了立刻写剪贴板。
    pub(crate) fn copy_image(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            self.set_status(self.language.copy_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.copy_no_images(), true, cx);
            return;
        };
        if self.export_in_flight {
            self.set_status(self.language.generating(), false, cx);
            return;
        }
        self.export_in_flight = true;
        self.export_generation = self.export_generation.wrapping_add(1);
        let generation = self.export_generation;
        let epoch = self.session_epoch;
        let size = job.output_size();
        self.set_status(self.language.generating(), false, cx);
        cx.spawn(async move |this, cx| {
            let encoded = cx
                .background_executor()
                .spawn(async move { job.encode_bytes() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.export_generation != generation {
                    return;
                }
                this.export_in_flight = false;
                if this.session_epoch != epoch {
                    return;
                }
                match encoded {
                    Ok(bytes) => this.write_image_to_clipboard(bytes, size, cx),
                    Err(err) => this.set_status(this.language.generate_failed(&err), true, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn write_image_to_clipboard(
        &mut self,
        bytes: Vec<u8>,
        size: (u32, u32),
        cx: &mut Context<Self>,
    ) {
        use gpui::{Image, ImageFormat};

        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            bytes,
        )));
        let message = format!("{} · {}×{}", self.language.copied(), size.0, size.1);
        self.set_status(message, false, cx);
        self.mark_success_flash("capsule-copy");
        self.maybe_start_clear_countdown(cx);
    }
}
