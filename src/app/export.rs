//! 导出 PNG 文件、复制图片到剪贴板。

use std::sync::Arc;

use gpui::{Context, Window};
use image::RgbImage;

use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::layout::{self, LayoutResult};
use crate::imaging::render;
use crate::model::{GapSetting, Mode};
use crate::platform;

use super::actions::{CopyImage, ExportPng};
use super::AppRoot;

/// 一次导出的内容。
enum ExportJob {
    /// 字幕模式：每张图 + 要保留的纵向范围
    Subtitle {
        parts: Vec<(Arc<RgbImage>, CropBand)>,
        gap: GapSetting,
    },
    /// 排版模式：图片 + 排版结果
    Layout {
        images: Vec<Arc<RgbImage>>,
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
            ExportJob::Layout {
                images,
                layout,
                gap,
            } => {
                let refs: Vec<&RgbImage> = images.iter().map(|image| image.as_ref()).collect();
                render::compose_layout(&refs, layout, gap.color)
            }
        }
    }

    fn file_name(&self) -> String {
        let kind = match self {
            ExportJob::Subtitle { .. } => "拼接",
            ExportJob::Layout { .. } => "排版",
        };
        format!(
            "视频截图_{kind}_{}.png",
            platform::timestamp()
        )
    }
}

impl AppRoot {
    fn build_export(&self) -> Option<ExportJob> {
        let inclusions = crop::resolve(&self.project);
        let gap = self.project.settings.gap;

        match self.project.mode {
            Mode::Subtitle => {
                let mut parts = Vec::new();
                for (index, item) in self.project.items.iter().enumerate() {
                    match inclusions.get(index).copied() {
                        Some(Inclusion::Crop(band)) => {
                            parts.push((item.rgb.clone(), band));
                        }
                        Some(Inclusion::FullImage) => {
                            parts.push((
                                item.rgb.clone(),
                                CropBand {
                                    y0: 0,
                                    y1: item.height,
                                },
                            ));
                        }
                        _ => {}
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(ExportJob::Subtitle { parts, gap })
                }
            }
            Mode::Layout => {
                let mut images = Vec::new();
                for (index, item) in self.project.items.iter().enumerate() {
                    if matches!(
                        inclusions.get(index),
                        Some(Inclusion::Crop(_)) | Some(Inclusion::FullImage)
                    ) {
                        images.push(item.rgb.clone());
                    }
                }
                if images.is_empty() {
                    return None;
                }
                let sizes: Vec<(u32, u32)> = images
                    .iter()
                    .map(|image| (image.width(), image.height()))
                    .collect();
                let layout = layout::arrange(
                    &sizes,
                    self.project.settings.layout,
                    gap.effective_px(),
                );
                Some(ExportJob::Layout {
                    images,
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

    fn export_png(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            self.set_status("还在处理图片，请稍等一下再导出", true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status("没有可导出的图片，先粘贴或打开几张图片", true, cx);
            return;
        };
        let file_name = job.file_name();
        self.set_status("正在生成图片…", false, cx);

        let task = cx.background_executor().spawn(async move {
            let image = job.render();
            render::encode_png(&image).map_err(|err| err.to_string())
        });

        cx.spawn(async move |this, cx| {
            let encoded = match task.await {
                Ok(bytes) => bytes,
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(format!("生成图片失败：{err}"), true, cx)
                    });
                    return;
                }
            };

            let directory = platform::default_save_dir();
            let receiver = match this.update(cx, |_this, cx| {
                cx.prompt_for_new_path(&directory, Some(&file_name))
            }) {
                Ok(receiver) => receiver,
                Err(_) => return,
            };

            match receiver.await {
                Ok(Ok(Some(path))) => {
                    let path_for_write = path.clone();
                    let write_task = cx.background_executor().spawn(async move {
                        std::fs::write(&path_for_write, encoded).map_err(|err| err.to_string())
                    });
                    let written = write_task.await;
                    let _ = this.update(cx, |this, cx| match written {
                        Ok(()) => this.set_status(
                            format!("已保存：{}", path.display()),
                            false,
                            cx,
                        ),
                        Err(err) => this.set_status(format!("保存失败：{err}"), true, cx),
                    });
                }
                Ok(Ok(None)) => {
                    let _ = this.update(cx, |this, cx| this.set_status("已取消保存", false, cx));
                }
                Ok(Err(err)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(format!("保存失败：{err}"), true, cx)
                    });
                }
                Err(_) => {}
            }
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

    #[cfg(target_os = "linux")]
    fn copy_image(&mut self, cx: &mut Context<Self>) {
        self.set_status(
            "Linux 上 GPUI 只能把文字写进系统剪贴板，图片请用「导出 PNG」保存",
            true,
            cx,
        );
    }

    #[cfg(not(target_os = "linux"))]
    fn copy_image(&mut self, cx: &mut Context<Self>) {
        use gpui::{Image, ImageFormat};

        if self.is_busy() {
            self.set_status("还在处理图片，请稍等一下再复制", true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status("没有可复制的图片，先粘贴或打开几张图片", true, cx);
            return;
        };
        self.set_status("正在生成图片…", false, cx);

        let task = cx.background_executor().spawn(async move {
            let image = job.render();
            render::encode_png(&image).map_err(|err| err.to_string())
        });

        cx.spawn(async move |this, cx| {
            match task.await {
                Ok(bytes) => {
                    let _ = this.update(cx, |this, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&Image::from_bytes(
                            ImageFormat::Png,
                            bytes,
                        )));
                        this.set_status("已复制到剪贴板", false, cx);
                    });
                }
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(format!("生成图片失败：{err}"), true, cx)
                    });
                }
            }
        })
        .detach();
    }
}
