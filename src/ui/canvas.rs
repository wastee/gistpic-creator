//! 画布：字幕模式与排版模式的实时预览，以及画布内的直接手势。
//!
//! 画布上没有常驻按钮：悬停看透视、拉裁剪线、拖接缝、拉框多选，
//! 所有操作都在图片和留白上完成。

mod geometry;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, CursorStyle, DragMoveEvent, IntoElement, MouseButton, MouseDownEvent,
    ObjectFit, SharedString, StyledImage, Window, div, img, px,
};

use crate::app::{AppRoot, CropHandleDrag, DragItem, ManualEdge, PREVIEW_TARGET_WIDTH, SeamDrag};
use crate::imaging::crop::CropBand;
use crate::model::Mode;
use crate::ui::ghost_crop;
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, CROP_HOTZONE, radius, size, space, text_size};
use crate::ui::widgets;

pub(crate) use geometry::{CanvasEntry, CanvasLayout, Rect, Seam, SeamAxis, marquee_hits};

impl AppRoot {
    /// 画布根节点：整块窗口都是画布，只有滚动条和底部胶囊浮在上面。
    pub(crate) fn render_canvas(
        &mut self,
        window: &Window,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let empty = self.project.items.is_empty();
        let layout = self.canvas_layout();
        let viewport = window.viewport_size();
        let origin = layout.origin(viewport, self.canvas_scroll.offset());
        let crop_top_room = self
            .crop_drag
            .as_ref()
            .filter(|drag| drag.edge == crate::app::ManualEdge::Top)
            .and_then(|drag| {
                self.project
                    .items
                    .iter()
                    .find(|item| item.id == drag.id)
                    .map(|item| drag.start.top * item.height as f32 * layout.scale)
            })
            .unwrap_or(0.0);
        let body_top = f32::from(space::XL) + crop_top_room;

        let body = if empty {
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(140.))
                .child(self.render_empty_state(theme, cx))
        } else {
            let mut body = div()
                .relative()
                .w_full()
                .pt(px(body_top))
                .pb(size::CANVAS_BOTTOM_PAD)
                .flex()
                .flex_col()
                .items_center()
                .child(self.render_stack(theme, &layout, cx));
            if let Some(marquee) = self.marquee.as_ref() {
                let rect = Rect::from_corners(marquee.start, marquee.current);
                body = body.child(
                    div()
                        .debug_selector(|| "marquee".to_string())
                        .absolute()
                        .left(px(origin.x + rect.x))
                        .top(px(body_top + rect.y))
                        .w(px(rect.w.max(1.0)))
                        .h(px(rect.h.max(1.0)))
                        .rounded(px(3.))
                        .bg(theme.accent.opacity(0.08))
                        .border_1()
                        .border_color(theme.accent.opacity(0.55)),
                );
            }
            body
        };

        let mut root = div()
            .debug_selector(|| "canvas".to_string())
            .id("canvas")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .relative()
            .overflow_scroll()
            .track_scroll(&self.canvas_scroll)
            .bg(theme.bg)
            .on_scroll_wheel(cx.listener(Self::canvas_scroll_wheel))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::canvas_right_click))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::canvas_mouse_down))
            .on_mouse_move(cx.listener(Self::canvas_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .child(div().w_full().flex().flex_col().items_center().child(body));

        if self.zoom != 1.0 {
            root = root.child(self.render_zoom_chip(theme, cx));
        }
        root
    }

    fn render_empty_state(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        div()
            .debug_selector(|| "empty-card".to_string())
            .id("empty-card")
            .w_full()
            .max_w(size::EMPTY_CARD_MAX)
            .mx(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(space::MD)
            .px(px(28.))
            .py(px(32.))
            .rounded(radius::CARD)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_float())
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_elevated))
            .on_click(
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.open_images(cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(56.))
                    .h(px(56.))
                    .rounded(px(16.))
                    .bg(theme.accent.opacity(0.10))
                    .child(icon::icon(Icon::PlusLarge, px(24.), theme.accent)),
            )
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::TITLE)
                    .font_weight(tokens::weight::STRONG)
                    .text_color(theme.text)
                    .child(language.empty_short()),
            )
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.muted)
                    .text_center()
                    .child(language.empty_card_hint()),
            )
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.faint)
                    .child(language.supported_formats()),
            )
    }

    fn render_stack(
        &mut self,
        theme: Theme,
        layout: &CanvasLayout,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let gap = self.project.settings.gap;
        let gap_enabled = gap.enabled && gap.px > 0;
        let mut stack = div()
            .debug_selector(|| "artwork".to_string())
            .id("artwork")
            .relative()
            .w(px(layout.width))
            .h(px(layout.height))
            .rounded(radius::WORK_CANVAS)
            .bg(if gap_enabled {
                widgets::quad_color(gap.color)
            } else {
                theme.surface
            })
            .when(!gap_enabled, |this| this.shadow(theme.shadow_work()));

        let entries = layout.entries.clone();
        let focused_id = self
            .crop_drag
            .as_ref()
            .map(|drag| drag.id)
            .or(self.hover_item);
        let mut ordered: Vec<&CanvasEntry> = entries.iter().collect();
        // 正在透视的那张最后画，幽灵层才能盖住相邻图段。
        ordered.sort_by_key(|entry| u8::from(Some(entry.id) != focused_id));
        for entry in ordered {
            if let Some(item) = self
                .project
                .items
                .iter()
                .find(|item| item.id == entry.id)
                .cloned()
            {
                stack = stack.child(self.render_block(theme, layout, entry, &item, cx));
            }
        }

        for seam in &layout.seams {
            stack = stack.child(self.render_seam(theme, seam, cx));
        }

        if self.dragging.is_some()
            && let Some(target) = self.drag_over
        {
            let (x, y, width) = match target {
                Some(id) => layout
                    .entry(id)
                    .map(|entry| {
                        (
                            entry.rect.x,
                            entry.rect.y - layout.gap_display / 2.0 - 1.0,
                            entry.rect.w,
                        )
                    })
                    .unwrap_or((0.0, 0.0, layout.width)),
                None => (0.0, layout.height + 2.0, layout.width),
            };
            stack = stack.child(
                div()
                    .debug_selector(|| "insert-line".to_string())
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(width))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(theme.accent),
            );
        }

        if layout.mode == Mode::Layout {
            stack = stack.child(self.render_canvas_corner(theme, layout, cx));
        }

        if let Some(drag) = self.gap_drag.as_ref()
            && let Some(seam) = layout.seams.iter().find(|seam| seam.index == drag.index)
        {
            let label = self
                .language
                .gap_px_label(self.project.settings.gap.effective_px());
            let (left, top) = match seam.axis {
                SeamAxis::Vertical => ((layout.width - 64.0).max(8.0), seam.center - 30.0),
                SeamAxis::Horizontal => (seam.center + 10.0, seam.rect.y),
            };
            stack = stack.child(
                div()
                    .debug_selector(|| "gap-bubble".to_string())
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(radius::PILL)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border_subtle)
                    .shadow(theme.shadow_bubble())
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.muted)
                    .child(label),
            );
        }
        stack
    }

    fn render_canvas_corner(
        &mut self,
        theme: Theme,
        layout: &CanvasLayout,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let handle = f32::from(size::RESIZE_HANDLE);
        div()
            .debug_selector(|| "canvas-corner".to_string())
            .id("canvas-corner")
            .absolute()
            .left(px((layout.width - handle).max(0.0)))
            .top(px((layout.height - handle).max(0.0)))
            .w(size::RESIZE_HANDLE)
            .h(size::RESIZE_HANDLE)
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .cursor(CursorStyle::ResizeUpLeftDownRight)
            .on_drag(
                crate::app::CanvasCornerDrag,
                move |_value, _offset, _window, cx| cx.new(|_| CropDragGhost),
            )
            .on_drag_move::<crate::app::CanvasCornerDrag>(cx.listener(
                |this: &mut AppRoot,
                 event: &DragMoveEvent<crate::app::CanvasCornerDrag>,
                 _window,
                 cx| {
                    this.drag_canvas_corner_handle(event, cx);
                },
            ))
            .child(
                div()
                    .w(px(10.))
                    .h(px(10.))
                    .border_r_2()
                    .border_b_2()
                    .border_color(theme.muted)
                    .rounded(px(2.)),
            )
    }

    fn render_block(
        &mut self,
        theme: Theme,
        layout: &CanvasLayout,
        entry: &CanvasEntry,
        item: &crate::model::Item,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let selected = self.is_selected(id);
        let focused = self.hover_item == Some(id)
            || self.crop_drag.as_ref().is_some_and(|drag| drag.id == id);
        let croppable = item.block.is_some()
            && matches!(item.status, crate::model::ItemStatus::Ready)
            && layout.mode == Mode::Subtitle;
        let scale = entry.display_width / item.width.max(1) as f32;

        let drag = self.crop_drag.as_ref().filter(|drag| drag.id == id);
        let band = entry.band;
        let dragging_crop = drag.is_some();
        let anchor_y0 = drag
            .map(|drag| (drag.start.top * item.height as f32) as u32)
            .unwrap_or(band.y0);
        let frame_height = if dragging_crop {
            item.height as f32 * scale
        } else {
            entry.display_height
        };
        let block_top = drag
            .map(|drag| drag.source_origin_y)
            .unwrap_or(entry.rect.y);
        let image_offset = 0.0;
        let image_height = if dragging_crop {
            item.height as f32 * scale
        } else {
            band.height() as f32 * scale
        };
        let (tl, tr, bl, br) = block_radii(layout, entry, layout.gap_px > 0);

        let full_preview = if focused && croppable {
            Some(self.preview_for(
                id,
                item.rgb.as_ref(),
                CropBand {
                    y0: 0,
                    y1: item.height,
                },
                PREVIEW_TARGET_WIDTH,
            ))
        } else {
            None
        };
        let preview = if dragging_crop {
            full_preview.clone().unwrap_or_else(|| {
                self.preview_for(id, item.rgb.as_ref(), band, PREVIEW_TARGET_WIDTH)
            })
        } else {
            self.preview_for(id, item.rgb.as_ref(), band, PREVIEW_TARGET_WIDTH)
        };

        let mut block = div()
            .debug_selector(move || format!("canvas-block-{id}"))
            .id(SharedString::from(format!("canvas-block-{id}")))
            .absolute()
            .left(px(entry.rect.x))
            .top(px(block_top))
            .w(px(entry.display_width))
            .h(px(frame_height))
            .cursor_pointer()
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover_item(id, *hovered, cx);
                }),
            )
            .on_click(
                cx.listener(move |this: &mut AppRoot, event: &ClickEvent, _window, cx| {
                    this.close_menu(cx);
                    this.select_with_modifiers(id, event.modifiers(), cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(
                    move |this: &mut AppRoot, event: &MouseDownEvent, _window, cx| {
                        this.open_image_menu(
                            id,
                            f32::from(event.position.x),
                            f32::from(event.position.y),
                            cx,
                        );
                    },
                ),
            )
            .on_drag(DragItem { id }, move |_value, _offset, _window, cx| {
                cx.new(|_| CropDragGhost)
            })
            .on_drag_move::<DragItem>(cx.listener(
                move |this: &mut AppRoot, event: &DragMoveEvent<DragItem>, _window, cx| {
                    let Some(dragged) = event.dragged_item().downcast_ref::<DragItem>().copied()
                    else {
                        return;
                    };
                    if dragged.id != id {
                        return;
                    }
                    let cursor = f32::from(event.event.position.y);
                    let top = f32::from(event.bounds.origin.y);
                    let height = f32::from(event.bounds.size.height);
                    let target = if cursor > top + height / 2.0 {
                        this.next_item_after(id)
                    } else {
                        Some(id)
                    };
                    this.set_drag_target(dragged.id, target, cx);
                },
            ))
            .on_drop(
                cx.listener(move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                    this.apply_drop(dragged.id, Some(id), cx);
                    this.clear_drag_state(cx);
                }),
            );

        if let Some(full) = full_preview {
            if dragging_crop {
                block = block.child(
                    img(full.clone())
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .w(px(entry.display_width))
                        .h(px(item.height as f32 * scale))
                        .object_fit(ObjectFit::Fill),
                );
            }
            block = block.child(ghost_crop::render_ghost_layers(
                item,
                band,
                if dragging_crop { 0 } else { anchor_y0 },
                entry.display_width,
                full,
                theme,
            ));
        }

        if !dragging_crop {
            block = block.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(image_offset))
                    .w(px(entry.display_width))
                    .h(px(image_height))
                    .overflow_hidden()
                    .rounded_tl(px(tl))
                    .rounded_tr(px(tr))
                    .rounded_bl(px(bl))
                    .rounded_br(px(br))
                    .child(
                        img(preview)
                            .w(px(entry.display_width))
                            .h(px(image_height))
                            .object_fit(ObjectFit::Fill),
                    ),
            );
        }

        if selected {
            block = block.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(image_offset))
                    .w(px(entry.display_width))
                    .h(px(image_height))
                    .rounded_tl(px(tl))
                    .rounded_tr(px(tr))
                    .rounded_bl(px(bl))
                    .rounded_br(px(br))
                    .border_2()
                    .border_color(theme.accent),
            );
            block = block.child(
                div()
                    .absolute()
                    .right(px(6.))
                    .top(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(18.))
                    .h(px(18.))
                    .rounded(px(9.))
                    .bg(theme.accent)
                    .child(icon::icon(Icon::Check, px(11.), theme.accent_text)),
            );
        }

        if croppable && let Some(drag) = self.crop_drag.as_ref().filter(|drag| drag.id == id) {
            block = block.child(ghost_crop::render_snap_ticks(
                item,
                drag.edge,
                0,
                entry.display_width,
                scale,
                self.project.settings.bleed,
                theme,
            ));
        }

        if croppable && focused {
            let crop_edge = self
                .crop_drag
                .as_ref()
                .filter(|drag| drag.id == id)
                .map(|drag| drag.edge)
                .unwrap_or(crate::app::ManualEdge::Top);
            block = block.child(ghost_crop::render_crop_lines(
                ghost_crop::CropLineView {
                    band,
                    edge: crop_edge,
                    anchor_y0: if dragging_crop { 0 } else { anchor_y0 },
                    display_width: entry.display_width,
                    scale,
                },
                theme,
                self.crop_drag.as_ref().and_then(|drag| drag.snap),
                self.language,
            ));
        }

        if croppable {
            let top_y = if dragging_crop {
                band.y0 as f32 * scale
            } else {
                0.0
            };
            let bottom_y = if dragging_crop {
                band.y1 as f32 * scale
            } else {
                frame_height
            };
            block = block
                .child(self.render_crop_hotzone(theme, entry, ManualEdge::Top, top_y, cx))
                .child(self.render_crop_hotzone(theme, entry, ManualEdge::Bottom, bottom_y, cx));
        }
        block
    }

    fn render_crop_hotzone(
        &mut self,
        _theme: Theme,
        entry: &CanvasEntry,
        edge: ManualEdge,
        line_y: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let top = line_y - f32::from(CROP_HOTZONE) / 2.0;
        div()
            .debug_selector(move || {
                format!(
                    "crop-hotzone-{id}-{}",
                    match edge {
                        ManualEdge::Top => "top",
                        ManualEdge::Bottom => "bottom",
                    }
                )
            })
            .id(SharedString::from(format!(
                "crop-hotzone-{id}-{}",
                match edge {
                    ManualEdge::Top => "top",
                    ManualEdge::Bottom => "bottom",
                }
            )))
            .absolute()
            .left(px(0.))
            .top(px(top))
            .w(px(entry.display_width))
            .h(CROP_HOTZONE)
            .cursor(CursorStyle::ResizeUpDown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(
                    move |this: &mut AppRoot, event: &MouseDownEvent, window, cx| {
                        this.begin_crop_drag(CropHandleDrag { id, edge }, event, window, cx);
                    },
                ),
            )
            .on_drag(
                CropHandleDrag { id, edge },
                move |_value, _offset, _window, cx| cx.new(|_| CropDragGhost),
            )
            .on_drag_move::<CropHandleDrag>(cx.listener(
                move |this: &mut AppRoot, event: &DragMoveEvent<CropHandleDrag>, window, cx| {
                    let Some(handle) = event
                        .dragged_item()
                        .downcast_ref::<CropHandleDrag>()
                        .copied()
                    else {
                        return;
                    };
                    this.drag_crop_handle(handle, event, window, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseUpEvent, _window, cx| {
                    this.end_crop_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseUpEvent, _window, cx| {
                    this.end_crop_drag(cx);
                }),
            )
    }

    fn render_seam(
        &mut self,
        theme: Theme,
        seam: &Seam,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let index = seam.index;
        let hovered = self.hover_seam == Some(index);
        let axis = seam.axis;
        let cursor = match axis {
            SeamAxis::Vertical => CursorStyle::ResizeUpDown,
            SeamAxis::Horizontal => CursorStyle::ResizeLeftRight,
        };
        div()
            .debug_selector(move || format!("seam-{index}"))
            .id(SharedString::from(format!("seam-{index}")))
            .absolute()
            .left(px(seam.rect.x))
            .top(px(seam.rect.y))
            .w(px(seam.rect.w))
            .h(px(seam.rect.h))
            .block_mouse_except_scroll()
            .cursor(cursor)
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover_seam(index, *hovered, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(
                    move |this: &mut AppRoot, event: &MouseDownEvent, window, cx| {
                        this.begin_gap_drag(index, event, window, cx);
                    },
                ),
            )
            .on_drag(SeamDrag { index }, move |_value, _offset, _window, cx| {
                cx.new(|_| SeamDragGhost)
            })
            .on_drag_move::<SeamDrag>(cx.listener(
                move |this: &mut AppRoot, event: &DragMoveEvent<SeamDrag>, _window, cx| {
                    this.drag_seam(index, event, cx);
                },
            ))
            .on_click(
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.cycle_gap_color(cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseUpEvent, _window, cx| {
                    this.end_gap_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseUpEvent, _window, cx| {
                    this.end_gap_drag(cx);
                }),
            )
            .when(hovered, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .right(px(0.))
                        .top(px((seam.rect.h - 2.0) / 2.0))
                        .h(px(2.))
                        .bg(theme.accent.opacity(0.55)),
                )
            })
    }

    fn render_zoom_chip(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let percent = (self.zoom * 100.0).round() as u32;
        div()
            .debug_selector(|| "zoom-chip".to_string())
            .id("zoom-chip")
            .absolute()
            .right(px(20.))
            .bottom(px(20.))
            .px(px(10.))
            .py(px(5.))
            .rounded(radius::PILL)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(theme.muted)
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_elevated))
            .on_click(
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.action_zoom_reset(cx);
                }),
            )
            .child(format!(
                "{} · {}",
                language.zoom_percent(percent),
                language.zoom_reset_hint()
            ))
    }
}

/// 图块的圆角：有缝隙时每块都是 8；没有缝隙时只有整张作品的外角是 12。
fn block_radii(
    layout: &CanvasLayout,
    entry: &CanvasEntry,
    gap_enabled: bool,
) -> (f32, f32, f32, f32) {
    if gap_enabled {
        return (8.0, 8.0, 8.0, 8.0);
    }
    let outer = f32::from(radius::WORK_CANVAS);
    match layout.mode {
        Mode::Subtitle => {
            let first = layout.entries.first().map(|entry| entry.id) == Some(entry.id);
            let last = layout.entries.last().map(|entry| entry.id) == Some(entry.id);
            (
                if first { outer } else { 0.0 },
                if first { outer } else { 0.0 },
                if last { outer } else { 0.0 },
                if last { outer } else { 0.0 },
            )
        }
        Mode::Layout => {
            let eps = 0.75;
            let left = entry.rect.x <= eps;
            let right = entry.rect.right() >= layout.width - eps;
            let top = entry.rect.y <= eps;
            let bottom = entry.rect.bottom() >= layout.height - eps;
            (
                if left && top { outer } else { 0.0 },
                if right && top { outer } else { 0.0 },
                if left && bottom { outer } else { 0.0 },
                if right && bottom { outer } else { 0.0 },
            )
        }
    }
}

/// 拖拽时跟着鼠标的透明小方块：真正的反馈在画布上，这里什么都不画。
struct CropDragGhost;

impl gpui::Render for CropDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct SeamDragGhost;

impl gpui::Render for SeamDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
