//! 中间预览区：字幕拼接结果 / 排版结果。
//!
//! 负责点选、多选、拖动排序、双击打开浮层、右键弹菜单、Ctrl+滚轮缩放。

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, DragMoveEvent, IntoElement, MouseButton, ObjectFit, ScrollDelta,
    ScrollWheelEvent, SharedString, StyledImage, div, img, px,
};
use image::RgbImage;

use crate::app::{AppRoot, BASE_PREVIEW_WIDTH, DragItem, PREVIEW_TARGET_WIDTH};
use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::layout;
use crate::model::{ItemId, Mode};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, ZOOM_STEP, radius, size, space, text_size};
use crate::ui::widgets::{self, DragGhost};

/// 预览里一张图要显示的信息。
struct PreviewEntry {
    item_id: ItemId,
    source: Arc<RgbImage>,
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
        let mut body = div().flex().flex_col().items_center().gap(space::MD).w_full();

        if self.project.items.is_empty() {
            body = body.child(empty_state(theme, self.language));
        } else {
            match self.project.mode {
                Mode::Subtitle => body = body.child(self.subtitle_stack(theme, cx)),
                Mode::Layout => body = body.child(self.layout_canvas(theme, cx)),
            }
        }

        div()
            .debug_selector(|| "preview-scroll".to_string())
            .id("preview-scroll")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_scroll()
            .bg(theme.surface_sunken)
            .p(space::XL)
            .on_mouse_down_out(cx.listener(
                |this: &mut AppRoot, _: &gpui::MouseDownEvent, _window, cx| {
                    this.close_image_menu(cx);
                    this.close_more_menu(cx);
                },
            ))
            // Ctrl/Cmd + 滚轮缩放整个预览；不按修饰键时交给滚动
            .on_scroll_wheel(cx.listener(
                |this: &mut AppRoot, event: &ScrollWheelEvent, _window, cx| {
                    if !event.modifiers.control && !event.modifiers.platform {
                        return;
                    }
                    let lines = match event.delta {
                        ScrollDelta::Lines(point) => point.y,
                        ScrollDelta::Pixels(point) => f32::from(point.y) / 20.0,
                    };
                    if lines.abs() < f32::EPSILON {
                        return;
                    }
                    this.zoom_by(lines.signum() * ZOOM_STEP, cx);
                    cx.stop_propagation();
                },
            ))
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
                band,
                display_width: base_width,
                display_height,
            });
        }
        entries
    }

    fn subtitle_stack(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.subtitle_entries();
        let drag_label = self.language.drag_insert_before();
        let mut stack = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(space::MD)
            .w_full();
        if entries.is_empty() {
            return stack.child(empty_state(theme, self.language));
        }

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

        for (entry, image) in prepared.into_iter() {
            let id = entry.item_id;
            let selected = self.is_selected(id);
            let active = self.selected == Some(id);
            let multi_selected = self.selected_count() > 1;
            let drop_target = self.drag_over == Some(id);
            let border = if drop_target {
                theme.accent
            } else if active || selected {
                theme.text
            } else {
                gpui::transparent_black()
            };
            let ghost_theme = theme;

            let card = div()
                .debug_selector(move || format!("preview-block-{id}"))
                .id(SharedString::from(format!("preview-block-{id}")))
                .relative()
                .w(px(entry.display_width))
                .flex()
                .flex_col()
                .rounded(radius::SMALL)
                .border_2()
                .border_color(border)
                .bg(theme.surface)
                .overflow_hidden()
                .cursor_pointer()
                .on_click(cx.listener(
                    move |this: &mut AppRoot, event: &ClickEvent, _window, cx| {
                        // 双击直接打开浮层
                        if event.click_count() >= 2 {
                            this.open_zoom_layer(id, cx);
                        } else {
                            this.select_with_modifiers(id, event.modifiers(), cx);
                        }
                    },
                ))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(
                        move |this: &mut AppRoot,
                              event: &gpui::MouseDownEvent,
                              _window,
                              cx| {
                            let x = f32::from(event.position.x);
                            let y = f32::from(event.position.y);
                            this.open_image_menu(id, x, y, cx);
                        },
                    ),
                )
                .on_drop(cx.listener(
                    move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.apply_drop(dragged.id, Some(id), cx);
                    },
                ))
                .on_drag(DragItem { id }, move |_value, _offset, _window, cx| {
                    cx.new(|_| DragGhost {
                        label: SharedString::from(drag_label),
                        theme: ghost_theme,
                    })
                })
                .on_drag_move::<DragItem>(cx.listener(
                    move |this: &mut AppRoot, _event: &DragMoveEvent<DragItem>, _window, cx| {
                        this.set_drag_over(Some(id), cx);
                    },
                ))
                .child(
                    img(image)
                        .w(px(entry.display_width))
                        .h(px(entry.display_height))
                        .object_fit(ObjectFit::Fill),
                );

            // 多选时在右上角打一个勾，其余时候图上不放任何常驻标签。
            let card = if selected && multi_selected {
                card.child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(18.))
                        .h(px(18.))
                        .rounded(px(9.))
                        .bg(theme.text)
                        .child(icon::icon(Icon::Check, px(11.), theme.bg)),
                )
            } else {
                card
            };

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
                    .rounded(radius::SMALL)
                    .text_size(text_size::CAPTION)
                    .font_family(tokens::FONT_FAMILY)
                    .text_color(theme.faint)
                    .on_drop(
                        cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                            this.apply_drop(dragged.id, None, cx);
                        }),
                    )
                    .child(self.language.drop_to_end()),
            );
        }

        stack
    }

    // ------------------------------------------------------------------
    // 排版预览
    // ------------------------------------------------------------------

    fn layout_canvas(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let drag_label = self.language.drag_insert_before();
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
            return div().w_full().child(empty_state(theme, self.language));
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
            theme.surface
        };

        let mut canvas = div()
            .relative()
            .w(px(canvas_w))
            .h(px(canvas_h))
            .rounded(radius::SMALL)
            .bg(background)
            .child(self.canvas_corner_handle(canvas_w, canvas_h, theme, cx));

        let mut previews: Vec<Arc<gpui::RenderImage>> = Vec::new();
        for (item_id, rgb, _, height) in &entries {
            let band = CropBand { y0: 0, y1: *height };
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
                .on_click(cx.listener(
                    move |this: &mut AppRoot, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.open_zoom_layer(id, cx);
                        } else {
                            this.select(id, cx);
                        }
                    },
                ))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(
                        move |this: &mut AppRoot,
                              event: &gpui::MouseDownEvent,
                              _window,
                              cx| {
                            let x = f32::from(event.position.x);
                            let y = f32::from(event.position.y);
                            this.open_image_menu(id, x, y, cx);
                        },
                    ),
                )
                .on_drag(DragItem { id }, move |_value, _offset, _window, cx| {
                    cx.new(|_| DragGhost {
                        label: SharedString::from(drag_label),
                        theme: ghost_theme,
                    })
                })
                .on_drop(
                    cx.listener(move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.apply_drop(dragged.id, Some(id), cx);
                    }),
                )
                .on_drag_move::<DragItem>(cx.listener(
                    move |this: &mut AppRoot, _event: &DragMoveEvent<DragItem>, _window, cx| {
                        this.set_drag_over(Some(id), cx);
                    },
                ));
            canvas = canvas.child(
                cell.child(
                    img(previews[placement.item].clone())
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                ),
            );
        }

        let output = self.language.layout_output(
            self.project.settings.layout,
            result.canvas_w,
            result.canvas_h,
        );

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(space::MD)
            .w_full()
            .child(widgets::faint_text(output, theme))
            .child(canvas)
            .child(
                div()
                    .id("layout-drop-end")
                    .w(px(canvas_w.min(360.)))
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::SMALL)
                    .text_size(text_size::CAPTION)
                    .font_family(tokens::FONT_FAMILY)
                    .text_color(theme.faint)
                    .on_drop(
                        cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                            this.apply_drop(dragged.id, None, cx);
                        }),
                    )
                    .child(self.language.drop_to_end()),
            )
    }

    /// 画布右下角的小三角手柄：拖它换目标比例。
    fn canvas_corner_handle(
        &self,
        canvas_w: f32,
        canvas_h: f32,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("canvas-corner")
            .debug_selector(|| "canvas-corner".to_string())
            .absolute()
            .left(px((canvas_w - f32::from(size::RESIZE_HANDLE)).max(0.0)))
            .top(px((canvas_h - f32::from(size::RESIZE_HANDLE)).max(0.0)))
            .w(size::RESIZE_HANDLE)
            .h(size::RESIZE_HANDLE)
            .flex()
            .items_end()
            .justify_end()
            .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
            .on_drag(
                CanvasCornerDrag,
                move |_value, _offset, _window, cx| {
                    cx.new(|_| DragGhost {
                        label: SharedString::from("比例"),
                        theme,
                    })
                },
            )
            .on_drag_move::<CanvasCornerDrag>(cx.listener(
                |this: &mut AppRoot, event: &DragMoveEvent<CanvasCornerDrag>, _window, cx| {
                    let anchor = event.bounds.origin;
                    let width = f32::from(event.event.position.x) - f32::from(anchor.x);
                    let height = f32::from(event.event.position.y) - f32::from(anchor.y);
                    this.drag_canvas_corner(width, height, cx);
                },
            ))
            .child(
                // 用手柄本身画一个小三角（用边框拼）
                div()
                    .w(px(8.))
                    .h(px(8.))
                    .mb(px(2.))
                    .mr(px(2.))
                    .bg(theme.faint)
                    .rounded(px(1.)),
            )
    }
}

/// 拖动画布右下角时携带的数据。
struct CanvasCornerDrag;

fn empty_state(theme: Theme, language: crate::i18n::Language) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(space::MD)
        .mt(px(90.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(96.))
                .h(px(72.))
                .rounded(radius::LARGE)
                .bg(theme.surface)
                .child(icon::icon(Icon::PlusLarge, px(28.), theme.faint)),
        )
        .child(
            div()
                .font_family(tokens::FONT_FAMILY)
                .text_size(text_size::TITLE)
                .text_color(theme.text)
                .child(language.empty_short()),
        )
        .child(widgets::hint_text(language.empty_hint(), theme))
        .child(widgets::faint_text(language.supported_formats(), theme))
}
