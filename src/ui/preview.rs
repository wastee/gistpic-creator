//! 中间预览区：字幕拼接结果 / 排版结果，支持拖动排序、点选、缩放。

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, img, px, ClickEvent, Context, DragMoveEvent, IntoElement, ObjectFit, SharedString,
    StyledImage,
};
use image::RgbImage;

use crate::app::{AppRoot, DragItem, BASE_PREVIEW_WIDTH, PREVIEW_TARGET_WIDTH};
use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::layout;
use crate::model::{ItemId, Mode};
use crate::ui::theme::Theme;
use crate::ui::widgets::{self, DragGhost};

/// 预览里一张图要显示的信息。
struct PreviewEntry {
    item_id: ItemId,
    source: Arc<RgbImage>,
    source_width: u32,
    band: CropBand,
    display_width: f32,
    display_height: f32,
}

impl AppRoot {
    pub(crate) fn render_preview(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut body = div().flex().flex_col().items_center().gap(px(14.)).w_full();

        if self.project.items.is_empty() {
            body = body.child(empty_state(theme));
        } else {
            match self.project.mode {
                Mode::Subtitle => body = body.child(self.subtitle_stack(theme, cx)),
                Mode::Layout => body = body.child(self.layout_canvas(theme, cx)),
            }
        }

        div()
            .id("preview-scroll")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_scroll()
            .bg(theme.preview_bg)
            .p(px(20.))
            .child(body)
    }

    // ------------------------------------------------------------------
    // 字幕拼接预览
    // ------------------------------------------------------------------

    fn subtitle_entries(&self) -> Vec<PreviewEntry> {
        let inclusions = crop::resolve(&self.project);
        let base_width = BASE_PREVIEW_WIDTH * self.zoom;
        let mut entries = Vec::new();

        for (index, item) in self.project.items.iter().enumerate() {
            let band = match inclusions.get(index).copied() {
                Some(Inclusion::Crop(band)) => band,
                Some(Inclusion::FullImage) => CropBand {
                    y0: 0,
                    y1: item.height,
                },
                _ => continue,
            };
            let display_height =
                (band.height() as f32 * base_width / item.width.max(1) as f32).max(1.0);
            entries.push(PreviewEntry {
                item_id: item.id,
                source: item.rgb.clone(),
                source_width: item.width,
                band,
                display_width: base_width,
                display_height,
            });
        }
        entries
    }

    fn subtitle_stack(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.subtitle_entries();
        let mut stack = div().flex().flex_col().items_center().gap(px(14.)).w_full();
        if entries.is_empty() {
            return stack.child(empty_state(theme));
        }

        let output = self
            .output_size()
            .map(|(width, height)| format!("输出 {width}×{height}"))
            .unwrap_or_default();
        stack = stack.child(
            div()
                .text_xs()
                .text_color(theme.muted)
                .child(format!("从上到下拼接 · {output}")),
        );

        let total = entries.len();
        let mut prepared: Vec<(PreviewEntry, Arc<gpui::RenderImage>)> = Vec::new();
        for entry in entries {
            let image = self.preview_for(
                entry.item_id,
                entry.source.as_ref(),
                entry.band,
                PREVIEW_TARGET_WIDTH,
            );
            prepared.push((entry, image));
        }

        for (position, (entry, image)) in prepared.into_iter().enumerate() {
            let id = entry.item_id;
            let selected = self.selected == Some(id);
            let drop_target = self.drag_over == Some(id);
            let border = if selected || drop_target {
                theme.accent
            } else {
                theme.border
            };
            let badge = format!(
                "#{} · {}×{}",
                position + 1,
                entry.source_width,
                entry.band.height()
            );
            let ghost_theme = theme;

            let card = div()
                .id(SharedString::from(format!("preview-block-{id}")))
                .relative()
                .w(px(entry.display_width))
                .flex()
                .flex_col()
                .rounded(px(8.))
                .border_2()
                .border_color(border)
                .bg(theme.block_bg)
                .overflow_hidden()
                .cursor_pointer()
                .on_click(cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.select(id, cx);
                }))
                .on_drag(DragItem { id }, move |_value, _offset, _window, cx| {
                    cx.new(|_| DragGhost {
                        label: SharedString::from("放到目标图片上，插到它前面"),
                        theme: ghost_theme,
                    })
                })
                .on_drop(cx.listener(
                    move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.apply_drop(dragged.id, Some(id), cx);
                    },
                ))
                .on_drag_move::<DragItem>(cx.listener(
                    move |this: &mut AppRoot,
                          _event: &DragMoveEvent<DragItem>,
                          _window,
                          cx| {
                        this.set_drag_over(Some(id), cx);
                    },
                ))
                .child(
                    img(image)
                        .w(px(entry.display_width))
                        .h(px(entry.display_height))
                        .object_fit(ObjectFit::Fill),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .left(px(6.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(theme.overlay_color())
                        .text_xs()
                        .text_color(gpui::white())
                        .child(badge),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(6.))
                        .right(px(6.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(theme.overlay_color())
                        .text_xs()
                        .text_color(gpui::white())
                        .child("拖动排序"),
                );

            stack = stack.child(card);
        }

        if total > 1 {
            stack = stack.child(
                div()
                    .id("preview-drop-end")
                    .w(px(BASE_PREVIEW_WIDTH * self.zoom))
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted)
                    .on_drop(cx.listener(
                        |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                            this.apply_drop(dragged.id, None, cx);
                        },
                    ))
                    .child("拖到这里，放到最后"),
            );
        }

        stack
    }

    // ------------------------------------------------------------------
    // 排版预览
    // ------------------------------------------------------------------

    fn layout_canvas(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let inclusions = crop::resolve(&self.project);
        let base_width = BASE_PREVIEW_WIDTH * self.zoom;
        let gap = self.project.settings.gap;

        let mut entries: Vec<(ItemId, Arc<RgbImage>, u32, u32)> = Vec::new();
        for (index, item) in self.project.items.iter().enumerate() {
            if matches!(
                inclusions.get(index),
                Some(Inclusion::Crop(_)) | Some(Inclusion::FullImage)
            ) {
                entries.push((item.id, item.rgb.clone(), item.width, item.height));
            }
        }
        if entries.is_empty() {
            return div().w_full().child(empty_state(theme));
        }

        let sizes: Vec<(u32, u32)> = entries
            .iter()
            .map(|(_, _, width, height)| (*width, *height))
            .collect();
        let result = layout::arrange(&sizes, self.project.settings.layout, gap.effective_px());
        let scale = if result.canvas_w == 0 {
            1.0
        } else {
            base_width / result.canvas_w as f32
        };
        let canvas_w = (result.canvas_w as f32 * scale).max(1.0);
        let canvas_h = (result.canvas_h as f32 * scale).max(1.0);

        let background = if gap.enabled {
            widgets::quad_color(gap.color)
        } else {
            theme.block_bg
        };

        let mut canvas = div()
            .relative()
            .w(px(canvas_w))
            .h(px(canvas_h))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(background);

        let mut previews: Vec<Arc<gpui::RenderImage>> = Vec::new();
        for (item_id, rgb, _, height) in &entries {
            let band = CropBand {
                y0: 0,
                y1: *height,
            };
            previews.push(self.preview_for(*item_id, rgb.as_ref(), band, PREVIEW_TARGET_WIDTH));
        }

        for placement in &result.placements {
            let Some((item_id, _, _, _)) = entries.get(placement.item) else {
                continue;
            };
            let id = *item_id;
            let selected = self.selected == Some(id);
            let drop_target = self.drag_over == Some(id);
            let border = if selected || drop_target {
                theme.accent
            } else {
                gpui::transparent_black()
            };
            let ghost_theme = theme;

            let cell = div()
                .id(SharedString::from(format!("layout-cell-{id}")))
                .absolute()
                .top(px(placement.y as f32 * scale))
                .left(px(placement.x as f32 * scale))
                .w(px(placement.w as f32 * scale))
                .h(px(placement.h as f32 * scale))
                .border_2()
                .border_color(border)
                .cursor_pointer()
                .on_click(cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.select(id, cx);
                }))
                .on_drag(DragItem { id }, move |_value, _offset, _window, cx| {
                    cx.new(|_| DragGhost {
                        label: SharedString::from("放到目标图片上，插到它前面"),
                        theme: ghost_theme,
                    })
                })
                .on_drop(cx.listener(
                    move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.apply_drop(dragged.id, Some(id), cx);
                    },
                ))
                .on_drag_move::<DragItem>(cx.listener(
                    move |this: &mut AppRoot,
                          _event: &DragMoveEvent<DragItem>,
                          _window,
                          cx| {
                        this.set_drag_over(Some(id), cx);
                    },
                ));
            canvas = canvas.child(cell.child(
                img(previews[placement.item].clone())
                    .size_full()
                    .object_fit(ObjectFit::Fill),
            ));
        }

        let output = format!(
            "{} · 输出 {}×{}",
            self.project.settings.layout.label(),
            result.canvas_w,
            result.canvas_h
        );

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .w_full()
            .child(div().text_xs().text_color(theme.muted).child(output))
            .child(canvas)
            .child(
                div()
                    .id("layout-drop-end")
                    .w(px(canvas_w.min(360.)))
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted)
                    .on_drop(cx.listener(
                        |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                            this.apply_drop(dragged.id, None, cx);
                        },
                    ))
                    .child("拖到这里，放到最后"),
            )
    }
}

fn empty_state(theme: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .mt(px(90.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(96.))
                .h(px(72.))
                .rounded(px(10.))
                .border_2()
                .border_color(theme.border)
                .text_lg()
                .text_color(theme.muted)
                .child("＋"),
        )
        .child(
            div()
                .text_lg()
                .text_color(theme.text)
                .child("把图片粘贴进来"),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted)
                .child("粘贴图片 · 拖入图片文件 · 或点顶部「打开图片」"),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted)
                .child("支持 PNG / JPEG / WebP / BMP / GIF"),
        )
}
