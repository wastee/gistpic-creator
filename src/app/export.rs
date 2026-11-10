//! 导出 PNG 文件、复制图片到剪贴板。

use std::sync::Arc;

use gpui::{Context, Window};
use image::RgbImage;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand, Inclusion};
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

    fn file_name(&self, language: Language) -> String {
        let kind = match self {
            ExportJob::Subtitle { .. } => language.export_kind_subtitle(),
            ExportJob::Layout { .. } => language.export_kind_layout(),
        };
        language.export_file_name(kind, &platform::timestamp())
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
                let layout =
                    layout::arrange(&sizes, self.project.settings.layout, gap.effective_px());
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
            self.set_status(self.language.export_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.export_no_images(), true, cx);
            return;
        };
        let file_name = job.file_name(self.language);
        self.set_status(self.language.generating(), false, cx);

        let task = cx.background_executor().spawn(async move {
            let image = job.render();
            render::encode_png(&image).map_err(|err| err.to_string())
        });

        cx.spawn(async move |this, cx| {
            let encoded = match task.await {
                Ok(bytes) => bytes,
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(this.language.generate_failed(&err), true, cx)
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
                            this.language.saved(&path.display().to_string()),
                            false,
                            cx,
                        ),
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

    fn copy_image(&mut self, cx: &mut Context<Self>) {
        use gpui::{Image, ImageFormat};

        if self.is_busy() {
            self.set_status(self.language.copy_busy(), true, cx);
            return;
        }
        let Some(job) = self.build_export() else {
            self.set_status(self.language.copy_no_images(), true, cx);
            return;
        };
        self.set_status(self.language.generating(), false, cx);

        let task = cx.background_executor().spawn(async move {
            let image = job.render();
            render::encode_png(&image).map_err(|err| err.to_string())
        });

        cx.spawn(async move |this, cx| match task.await {
            Ok(bytes) => {
                #[cfg(any(debug_assertions, feature = "dev-tools"))]
                log::info!("复制图片：PNG {} 字节，写入系统剪贴板", bytes.len());
                let _ = this.update(cx, |this, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&Image::from_bytes(
                        ImageFormat::Png,
                        bytes,
                    )));
                    this.set_status(this.language.copied(), false, cx);
                });
            }
            Err(err) => {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(this.language.generate_failed(&err), true, cx)
                });
            }
        })
        .detach();
    }
}
