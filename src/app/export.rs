//! 导出 PNG 文件、复制图片到剪贴板。

use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Window};
use image::RgbImage;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand};
use crate::imaging::layout::{self, LayoutResult};
use crate::imaging::render;
use crate::model::{GapSetting, Mode};
use crate::platform;

use super::AppRoot;
use super::actions::{CopyImage, ExportPng};

/// 一次导出的内容。
enum ExportJob {
    /// 字幕模式：每张图 + 要保留的纵向范围
    Subtitle {
        parts: Vec<(Arc<RgbImage>, CropBand)>,
        gap: GapSetting,
    },
    /// 排版模式：原图 + 待裁剪范围 + 排版结果。裁剪本身留到后台 render 里做。
    Layout {
        parts: Vec<(Arc<RgbImage>, CropBand)>,
        layout: LayoutResult,
        gap: GapSetting,
    },
}

impl ExportJob {
    fn render(&self) -> image::RgbaImage {
        match self {
            ExportJob::Subtitle { parts, gap } => {
                let refs: Vec<(&RgbImage, CropBand)> = parts
                    .iter()
                    .map(|(image, band)| (image.as_ref(), *band))
                    .collect();
                render::compose_subtitle(&refs, gap)
            }
            ExportJob::Layout { parts, layout, gap } => {
                // 排版按裁剪后的尺寸摆放，这里才真正裁出每块图；
                // 整个合成都在后台任务里，不占 UI 线程。
                let images: Vec<RgbImage> = parts
                    .iter()
                    .map(|(source, band)| render::crop_band_rgb(source.as_ref(), *band))
                    .collect();
                let refs: Vec<&RgbImage> = images.iter().collect();
                render::compose_layout(&refs, layout, gap.color)
            }
        }
    }

    fn file_name(&self, language: Language) -> String {
        let kind = match self {
            ExportJob::Subtitle { .. } => language.export_kind_subtitle(),
            ExportJob::Layout { .. } => language.export_kind_layout(),
        };
        language.export_file_name(kind, &platform::timestamp())
    }

    /// 这份导出内容的内容指纹，用来判断缓存能不能直接用。
    ///
    /// 图片位图放进项目后不会再改，所以用 `Arc` 指针代表「是哪张图」；
    /// 裁剪范围、缝隙、排版结果都是数值，直接算进去。
    fn signature(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_parts(hasher: &mut DefaultHasher, parts: &[(Arc<RgbImage>, CropBand)]) {
            parts.len().hash(hasher);
            for (image, band) in parts {
                Arc::as_ptr(image).hash(hasher);
                band.hash(hasher);
            }
        }

        fn hash_gap(hasher: &mut DefaultHasher, gap: &GapSetting) {
            gap.enabled.hash(hasher);
            gap.px.hash(hasher);
            gap.color.r.hash(hasher);
            gap.color.g.hash(hasher);
            gap.color.b.hash(hasher);
            gap.color.a.hash(hasher);
        }

        let mut hasher = DefaultHasher::new();
        match self {
            ExportJob::Subtitle { parts, gap } => {
                0u8.hash(&mut hasher);
                hash_parts(&mut hasher, parts);
                hash_gap(&mut hasher, gap);
            }
            ExportJob::Layout { parts, layout, gap } => {
                1u8.hash(&mut hasher);
                hash_parts(&mut hasher, parts);
                layout.canvas_w.hash(&mut hasher);
                layout.canvas_h.hash(&mut hasher);
                layout.placements.len().hash(&mut hasher);
                for placement in &layout.placements {
                    placement.item.hash(&mut hasher);
                    placement.x.hash(&mut hasher);
                    placement.y.hash(&mut hasher);
                    placement.w.hash(&mut hasher);
                    placement.h.hash(&mut hasher);
                }
                hash_gap(&mut hasher, gap);
            }
        }
        hasher.finish()
    }
}

/// 内容停下来多久之后才在后台开始算成品。
///
/// 拖裁剪线、拖接缝时内容每帧都在变，不能每帧都重算；停下约 150 毫秒后
/// 再算一次，用户改完到点复制按钮这段时间里一般已经算好了。
pub(crate) const OUTPUT_DEBOUNCE: Duration = Duration::from_millis(150);

/// 同时最多跑几个后台成品任务。
///
/// 平时只跟着内容变化排一个任务就够了；用户点了复制或导出、而旧任务还在算
/// 已经过时的内容时，允许再起一个算最新内容，免得用户等完旧的还要再等新的。
const MAX_OUTPUT_BUILDS: usize = 2;

/// 已经算好的成品 PNG，以及它对应的内容指纹。
pub(crate) struct OutputCache {
    /// 算出这份 PNG 时的内容指纹。
    pub(crate) signature: u64,
    /// 成品 PNG 字节，复制和导出共用这一份。
    pub(crate) bytes: Arc<Vec<u8>>,
}

impl AppRoot {
    fn build_export(&self) -> Option<ExportJob> {
        let inclusions = crop::resolve(&self.project);
        let gap = self.project.settings.gap;

        match self.project.mode {
            Mode::Subtitle => {
                let mut parts = Vec::new();
                for (index, item) in self.project.items.iter().enumerate() {
                    let Some(inclusion) = inclusions.get(index).copied() else {
                        continue;
                    };
                    if inclusion.output_size(item).is_some() {
                        parts.push((item.rgb.clone(), inclusion.band_for(item)));
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(ExportJob::Subtitle { parts, gap })
                }
            }
            Mode::Layout => {
                let mut parts = Vec::new();
                let mut sizes = Vec::new();
                for (index, item) in self.project.items.iter().enumerate() {
                    let Some(inclusion) = inclusions.get(index).copied() else {
                        continue;
                    };
                    let Some(size) = inclusion.output_size(item) else {
                        continue;
                    };
                    // 只收集 Arc 与元信息，裁剪与合成都交给后台的 render()。
                    parts.push((item.rgb.clone(), inclusion.band_for(item)));
                    sizes.push(size);
                }
                if parts.is_empty() {
                    return None;
                }
                let layout =
                    layout::arrange(&sizes, self.project.settings.layout, gap.effective_px());
                Some(ExportJob::Layout {
                    parts,
                    layout,
                    gap,
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

    /// 导出 PNG 文件。
    ///
    /// 成品已经提前算好时直接弹保存框；没算好就现在算，算完自动弹，
    /// 不用再点一次。
    pub(crate) fn export_png(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            self.set_status(self.language.export_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.export_no_images(), true, cx);
            return;
        };
        let file_name = job.file_name(self.language);
        if let Some(bytes) = self.fresh_output(job.signature()) {
            self.save_encoded_png(bytes, file_name, cx);
            return;
        }
        self.set_status(self.language.generating(), false, cx);
        self.output_pending_save = Some(file_name);
        // 用户主动点了导出：上次失败也再试一次。
        self.output_failed = None;
        self.start_output_build(true, cx);
    }

    pub(crate) fn action_copy(
        &mut self,
        _: &CopyImage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_image(cx);
    }

    /// 把成品复制到剪贴板。
    ///
    /// 内容稳定后成品已经在后台算好，这里只把现成的 PNG 字节写进剪贴板，
    /// 点下去就完成。缓存还没跟上时才开始算，算好立刻写，不用再按一次。
    pub(crate) fn copy_image(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            self.set_status(self.language.copy_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.copy_no_images(), true, cx);
            return;
        };
        if let Some(bytes) = self.fresh_output(job.signature()) {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::info!("复制图片：直接用算好的成品，{} 字节", bytes.len());
            self.write_image_to_clipboard(bytes, cx);
            return;
        }
        self.set_status(self.language.generating(), false, cx);
        self.output_wants_copy = true;
        // 用户主动点了复制：上次失败也再试一次。
        self.output_failed = None;
        self.start_output_build(true, cx);
    }

    /// 当前内容的成品指纹；没有可导出的图片时为 None。
    pub(crate) fn export_signature(&self) -> Option<u64> {
        self.build_export().map(|job| job.signature())
    }

    /// 缓存里的成品和这份指纹对得上就返回，对不上返回 None。
    pub(crate) fn fresh_output(&self, signature: u64) -> Option<Arc<Vec<u8>>> {
        self.output_cache
            .as_ref()
            .filter(|cache| cache.signature == signature)
            .map(|cache| cache.bytes.clone())
    }

    /// 每帧调用：内容变了就在后台提前把成品算好。
    ///
    /// 用户点复制时能直接取缓存，不用等合成。内容改得很快时先等一小段
    /// 稳定时间再算，避免拖裁剪线、拖接缝时每帧都白算一遍。
    pub(crate) fn ensure_output_cache(&mut self, cx: &mut Context<Self>) {
        // 正在解码或识别时不抢 CPU：忙完会重画，那时再算成品。
        if self.is_busy() {
            return;
        }
        let Some(job) = self.build_export() else {
            self.output_cache = None;
            if self.output_debounce_pending {
                self.cancel_output_debounce();
            }
            self.output_wants_copy = false;
            self.output_pending_save = None;
            return;
        };
        let signature = job.signature();
        if let Some(bytes) = self.fresh_output(signature) {
            if self.output_debounce_pending {
                self.cancel_output_debounce();
            }
            self.serve_output_requests(bytes, cx);
            return;
        }
        // 这一版内容已经生成失败过：等内容变了再自动重试。
        if self.output_failed == Some(signature) {
            return;
        }
        if self.output_wants_copy || self.output_pending_save.is_some() {
            if self.output_in_flight != Some(signature) {
                self.start_output_build(true, cx);
            }
            return;
        }
        if self.output_debounce_pending || self.output_in_flight == Some(signature) {
            return;
        }
        self.schedule_output_debounce(signature, cx);
    }

    fn cancel_output_debounce(&mut self) {
        self.output_debounce_pending = false;
        self.output_debounce_gen = self.output_debounce_gen.wrapping_add(1);
    }

    fn schedule_output_debounce(&mut self, signature: u64, cx: &mut Context<Self>) {
        self.output_debounce_pending = true;
        self.output_debounce_gen = self.output_debounce_gen.wrapping_add(1);
        let generation = self.output_debounce_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(OUTPUT_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                if this.output_debounce_gen != generation {
                    return;
                }
                this.output_debounce_pending = false;
                // 又忙起来了（比如刚粘了新图）：等忙完的那次重画再安排。
                if this.is_busy() {
                    return;
                }
                // 等待的这段时间内容又变了：让下一帧重新等一个完整间隔。
                if this.export_signature() != Some(signature) {
                    cx.notify();
                    return;
                }
                this.start_output_build(false, cx);
            });
        })
        .detach();
    }

    /// 立刻在后台算成品。
    ///
    /// 已经有任务在算同一版内容时直接复用；urgent 为真（用户点了复制、导出）
    /// 时允许和旧任务并行，最多同时跑两个。
    fn start_output_build(&mut self, urgent: bool, cx: &mut Context<Self>) {
        self.cancel_output_debounce();
        let Some(job) = self.build_export() else {
            self.output_wants_copy = false;
            self.output_pending_save = None;
            return;
        };
        let signature = job.signature();
        if let Some(bytes) = self.fresh_output(signature) {
            self.serve_output_requests(bytes, cx);
            return;
        }
        if self.output_in_flight == Some(signature) {
            return;
        }
        let limit = if urgent { MAX_OUTPUT_BUILDS } else { 1 };
        if self.output_builds_running >= limit {
            return;
        }
        self.output_in_flight = Some(signature);
        self.output_builds_running += 1;
        let task = cx.background_executor().spawn(async move {
            let image = job.render();
            render::encode_png(&image).map_err(|err| err.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.output_builds_running = this.output_builds_running.saturating_sub(1);
                if this.output_in_flight == Some(signature) {
                    this.output_in_flight = None;
                }
                match result {
                    Ok(bytes) => {
                        let bytes = Arc::new(bytes);
                        // 渲染期间内容可能又改了：只保存仍然对得上的那一版。
                        if this.export_signature() == Some(signature) {
                            this.output_cache = Some(OutputCache {
                                signature,
                                bytes: bytes.clone(),
                            });
                            this.output_failed = None;
                            this.serve_output_requests(bytes, cx);
                        }
                        cx.notify();
                    }
                    Err(err) => {
                        this.output_failed = Some(signature);
                        // 失败的是过时内容时，用户等的那份新结果可能还在算，别清掉请求。
                        if this.export_signature() == Some(signature) && this.take_output_requests()
                        {
                            this.set_status(this.language.generate_failed(&err), true, cx);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// 有没有人在等这份成品；有就清掉等待标记并返回 true。
    fn take_output_requests(&mut self) -> bool {
        let waiting = self.output_wants_copy || self.output_pending_save.is_some();
        self.output_wants_copy = false;
        self.output_pending_save = None;
        waiting
    }

    /// 成品算好之后，把等着的复制、导出请求各做一次。
    fn serve_output_requests(&mut self, bytes: Arc<Vec<u8>>, cx: &mut Context<Self>) {
        if self.output_wants_copy {
            self.output_wants_copy = false;
            self.write_image_to_clipboard(bytes.clone(), cx);
        }
        if let Some(file_name) = self.output_pending_save.take() {
            self.save_encoded_png(bytes, file_name, cx);
        }
    }

    fn write_image_to_clipboard(&mut self, bytes: Arc<Vec<u8>>, cx: &mut Context<Self>) {
        use gpui::{Image, ImageFormat};

        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            bytes.as_ref().clone(),
        )));
        let message = match self.output_size() {
            Some((width, height)) => format!("{} · {width}×{height}", self.language.copied()),
            None => self.language.copied().to_string(),
        };
        self.set_status(message, false, cx);
        self.mark_success_flash("capsule-copy");
    }

    fn save_encoded_png(&mut self, bytes: Arc<Vec<u8>>, file_name: String, cx: &mut Context<Self>) {
        let directory = platform::default_save_dir();
        let receiver = cx.prompt_for_new_path(&directory, Some(&file_name));

        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                let path_for_write = path.clone();
                let bytes = bytes.clone();
                let write_task = cx.background_executor().spawn(async move {
                    std::fs::write(&path_for_write, bytes.as_ref()).map_err(|err| err.to_string())
                });
                let written = write_task.await;
                let _ = this.update(cx, |this, cx| match written {
                    Ok(()) => {
                        this.set_status(
                            this.language.saved(&path.display().to_string()),
                            false,
                            cx,
                        );
                        this.mark_success_flash("export-png");
                    }
                    Err(err) => {
                        this.set_status(this.language.save_failed(&err.to_string()), true, cx)
                    }
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
}
