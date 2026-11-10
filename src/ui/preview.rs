//! 中间预览区：字幕拼接结果 / 排版结果，支持拖动排序、点选、缩放。

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, CursorStyle, DragMoveEvent, IntoElement, MouseButton, ObjectFit,
    SharedString, StyledImage, div, img, px,
};
use image::RgbImage;

use crate::app::{
    AppRoot, BASE_PREVIEW_WIDTH, DragItem, ManualDrag, ManualEdge, PREVIEW_TARGET_WIDTH,
};
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
    source_height: u32,
    /// 实际显示出来的纵向范围；框选时这里是整张图。
    band: CropBand,
    /// 真正会参与输出的裁剪范围，框选时用来画横线。
    crop_band: CropBand,
    display_width: f32,
    display_height: f32,
    editing: bool,
}

impl AppRoot {
    pub(crate) fn render_preview(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut body = div().flex().flex_col().items_center().gap(px(14.)).w_full();

        if self.project.items.is_empty() {
            body = body.child(empty_state(theme, self.language));
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
            let crop_band = match inclusions.get(index).copied() {
                Some(Inclusion::Crop(band)) => band,
                Some(Inclusion::FullImage) => CropBand {
                    y0: 0,
                    y1: item.height,
                },
                _ => continue,
            };
            let editing = self.crop_edit == Some(item.id);
            let band = if editing {
                CropBand {
                    y0: 0,
                    y1: item.height,
                }
            } else {
                crop_band
            };
            let display_height =
                (band.height() as f32 * base_width / item.width.max(1) as f32).max(1.0);
            entries.push(PreviewEntry {
                item_id: item.id,
                source: item.rgb.clone(),
                source_width: item.width,
                source_height: item.height,
                band,
                crop_band,
                display_width: base_width,
                display_height,
                editing,
            });
        }
        entries
    }

    fn subtitle_stack(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.subtitle_entries();
        let drag_label = self.language.drag_insert_before();
        let mut stack = div().flex().flex_col().items_center().gap(px(14.)).w_full();
        if entries.is_empty() {
            return stack.child(empty_state(theme, self.language));
        }

        let output = self
            .output_size()
            .map(|(width, height)| self.language.output_size(width, height))
            .unwrap_or_default();
        stack = stack.child(
            div()
                .text_xs()
                .text_color(theme.muted)
                .child(self.language.stitched_output(&output)),
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
            let selected = self.is_selected(id);
            let active = self.selected == Some(id);
            let multi_selected = self.selected_count() > 1;
            let editing = entry.editing;
            let drop_target = self.drag_over == Some(id);
            let border = if active || drop_target {
                theme.accent
            } else if selected {
                theme.muted
            } else {
                theme.border
            };
            let badge = format!(
                "#{} · {}×{}",
                position + 1,
                entry.source_width,
                entry.crop_band.height()
            );
            let ghost_theme = theme;
            let manual_label: SharedString = self.language.manual_crop().into();
            let top_drag_label = manual_label.clone();
            let bottom_drag_label = manual_label.clone();
            let source_height = entry.source_height.max(1) as f32;
            let top_ratio = entry.crop_band.y0 as f32 / source_height;
            let bottom_ratio = entry.crop_band.y1 as f32 / source_height;
            let top_y = (entry.display_height * top_ratio).clamp(0.0, entry.display_height);
            let bottom_y = (entry.display_height * bottom_ratio).clamp(0.0, entry.display_height);
            let band_height = (bottom_y - top_y).max(0.0);

            let mut card = div()
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
                .on_click(cx.listener(
                    move |this: &mut AppRoot, event: &ClickEvent, _window, cx| {
                        this.select_with_modifiers(id, event.modifiers(), cx);
                    },
                ))
                .on_drop(cx.listener(
                    move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.apply_drop(dragged.id, Some(id), cx);
                    },
                ));

            if !editing {
                card = card
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
                    ));
            } else {
                card = card
                    .on_drag_move::<ManualDrag>(cx.listener(
                        move |this: &mut AppRoot,
                              event: &DragMoveEvent<ManualDrag>,
                              _window,
                              cx| {
                            let edge = event.drag(cx).edge;
                            let ratio = (event.event.position.y - event.bounds.top())
                                / event.bounds.size.height;
                            this.update_manual_drag(id, edge, ratio, cx);
                        },
                    ))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this: &mut AppRoot, _: &gpui::MouseUpEvent, _window, _cx| {
                            this.end_manual_drag();
                        }),
                    );
            }

            card = card.child(
                img(image)
                    .w(px(entry.display_width))
                    .h(px(entry.display_height))
                    .object_fit(ObjectFit::Fill),
            );

            if editing {
                let top_dim = (entry.display_height * top_ratio).max(0.0);
                let bottom_dim = (entry.display_height - bottom_y).max(0.0);
                card = card
                    .child(
                        div()
                            .absolute()
                            .top(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(px(top_dim))
                            .bg(gpui::rgba(0x00000099)),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(px(bottom_dim))
                            .bg(gpui::rgba(0x00000099)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(top_y))
                            .left(px(0.))
                            .w_full()
                            .h(px(band_height))
                            .border_2()
                            .border_color(theme.accent),
                    )
                    .child(
                        div()
                            .debug_selector(|| format!("manual-top-{id}"))
                            .id(SharedString::from(format!("manual-top-{id}")))
                            .absolute()
                            .top(px(top_y))
                            .left(px(0.))
                            .w_full()
                            .h(px(12.))
                            .cursor(CursorStyle::ResizeUpDown)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(
                                    move |this: &mut AppRoot,
                                          _: &gpui::MouseDownEvent,
                                          _window,
                                          cx| {
                                        this.begin_manual_drag(id, ManualEdge::Top, cx);
                                    },
                                ),
                            )
                            .on_click(cx.listener(
                                |_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    cx.stop_propagation();
                                },
                            ))
                            .on_drag(
                                ManualDrag {
                                    id,
                                    edge: ManualEdge::Top,
                                },
                                move |_value, _offset, _window, cx| {
                                    cx.new(|_| DragGhost {
                                        label: top_drag_label.clone(),
                                        theme: ghost_theme,
                                    })
                                },
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .w_full()
                                    .h(px(2.))
                                    .bg(theme.accent),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| format!("manual-bottom-{id}"))
                            .id(SharedString::from(format!("manual-bottom-{id}")))
                            .absolute()
                            .top(px((bottom_y - 12.0).max(0.0)))
                            .left(px(0.))
                            .w_full()
                            .h(px(12.))
                            .cursor(CursorStyle::ResizeUpDown)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(
                                    move |this: &mut AppRoot,
                                          _: &gpui::MouseDownEvent,
                                          _window,
                                          cx| {
                                        this.begin_manual_drag(id, ManualEdge::Bottom, cx);
                                    },
                                ),
                            )
                            .on_click(cx.listener(
                                |_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    cx.stop_propagation();
                                },
                            ))
                            .on_drag(
                                ManualDrag {
                                    id,
                                    edge: ManualEdge::Bottom,
                                },
                                move |_value, _offset, _window, cx| {
                                    cx.new(|_| DragGhost {
                                        label: bottom_drag_label.clone(),
                                        theme: ghost_theme,
                                    })
                                },
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom(px(0.))
                                    .left(px(0.))
                                    .w_full()
                                    .h(px(2.))
                                    .bg(theme.accent),
                            ),
                    );
            }

            card = card
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
                        .child(if editing {
                            self.language.manual_crop()
                        } else {
                            self.language.drag_reorder()
                        }),
                );

            if selected && multi_selected {
                card = card.child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(18.))
                        .h(px(18.))
                        .rounded(px(9.))
                        .bg(theme.accent)
                        .text_xs()
                        .text_color(gpui::white())
                        .child("✓"),
                );
            }

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
                .on_click(
                    cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                        this.select(id, cx);
                    }),
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
                    .on_drop(
                        cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                            this.apply_drop(dragged.id, None, cx);
                        }),
                    )
                    .child(self.language.drop_to_end()),
            )
    }
}

fn empty_state(theme: Theme, language: crate::i18n::Language) -> impl IntoElement {
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
                .child(language.empty_title()),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted)
                .child(language.empty_hint()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted)
                .child(language.supported_formats()),
        )
}
