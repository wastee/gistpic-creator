//! 画布：字幕模式与排版模式的实时预览，以及画布内的直接手势。
//!
//! 画布上没有常驻按钮：悬停看透视、拉裁剪线、拖接缝、拉框多选，
//! 所有操作都在图片和留白上完成。

mod gap_control;
mod geometry;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, CursorStyle, DragMoveEvent,
    IntoElement, MouseButton, MouseDownEvent, ObjectFit, SharedString, StyledImage, Window, div,
    ease_out_quint, img, px,
};
use std::time::Duration;

use crate::app::{
    AppRoot, CropHandleDrag, DragItem, ManualEdge, PREVIEW_MAX_WIDTH, PREVIEW_MIN_WIDTH,
};
use crate::model::Mode;
use crate::ui::ghost_crop;
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, CANVAS_SIDE_PAD, CROP_HOTZONE, radius, size, space, text_size};
use crate::ui::widgets;

pub(crate) use geometry::{
    CanvasEntry, CanvasLayout, DragPreview, Rect, Seam, auto_scroll_intensity, marquee_hits,
};

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
        // GPU 按物理像素采样，预览位图必须按“显示宽 × 系统缩放”生成才不会发糊。
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let origin = layout.origin(viewport, self.canvas_scroll.offset());
        // 裁剪拖动期间留白已冻结在 layout 里，渲染与坐标换算共用同一份，
        // 不再为「上边拖动」额外让位，也就不会把后面的图片整体往下推。
        let body_top = layout.top_padding(viewport);

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
                .child(self.render_stack(theme, &layout, scale, cx));
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

        // 滚动区只有一个直接子元素，横向可滚距离是从它的宽度量出来的。
        // 作品比窗口宽时，这个子元素要跟着变宽，否则横向永远滚不动。
        let content_min_width = px(layout.width + 2.0 * f32::from(CANVAS_SIDE_PAD));
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
            .on_mouse_down(MouseButton::Right, cx.listener(Self::canvas_right_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::canvas_mouse_down))
            .on_mouse_move(cx.listener(Self::canvas_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::canvas_right_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::canvas_right_up_out))
            .child(
                div()
                    .w_full()
                    .min_w(content_min_width)
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(body),
            );

        if self.zoom != 1.0 {
            root = root.child(self.render_zoom_chip(theme, layout.scale, cx));
        }
        root
    }

    fn render_empty_state(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let fade = self.hover_gate.fade_delta(&HoverKey::widget("empty-card"));
        let card = div()
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
            .bg(hover::fade_background(
                Some(theme.surface),
                theme.surface_elevated,
                fade,
            ))
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_float())
            .cursor_pointer()
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("empty-card"), *hovered, cx);
                }),
            )
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
            );
        card
    }

    fn render_stack(
        &mut self,
        theme: Theme,
        layout: &CanvasLayout,
        device_scale: f32,
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
            .when(!gap_enabled, |this| this.shadow(theme.shadow_work()))
            // 拖动排序：被拖的那张此刻不在排版里，所以落点判定挂在整块作品上，
            // 按指针位置和剩下的图块算插到哪张前面。
            .on_drag_move::<DragItem>(cx.listener(
                move |this: &mut AppRoot, event: &DragMoveEvent<DragItem>, window, cx| {
                    let Some(dragged) = event.dragged_item().downcast_ref::<DragItem>().copied()
                    else {
                        return;
                    };
                    // 记录指针位置，跟手的半透明小图画在根节点最上层。
                    this.drag_pointer = Some(event.event.position);
                    // 按“已经摘掉被拖那张”的排版换算坐标和落点，第一帧就一致。
                    let target =
                        this.drag_target_for(event.event.position, window.viewport_size(), dragged.id);
                    this.set_drag_target(dragged.id, target, cx);
                    // 指针压到视口边缘就挂上自动滚动，这样停在边上也能继续翻。
                    this.refresh_auto_scroll(cx);
                    // 即使插入位置没变，指针动了也要重画，跟手小图才会跟着走。
                    cx.notify();
                },
            ))
            .on_drop(
                cx.listener(move |this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                    if let Some(before) = this.drag_over {
                        this.apply_drop(dragged.id, before, cx);
                    }
                    this.clear_drag_state(cx);
                }),
            );

        if gap_enabled && let Some(from_color) = self.prev_gap_color {
            let to_color = widgets::quad_color(gap.color);
            stack = stack.child(
                div()
                    .debug_selector(|| "gap-color-fade".to_string())
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(layout.width))
                    .h(px(layout.height))
                    .rounded(radius::WORK_CANVAS)
                    .bg(from_color)
                    .with_animation(
                        SharedString::from(format!("gap-color-{}", self.gap_color_gen)),
                        Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                        move |el, delta| {
                            el.bg(crate::ui::theme::lerp_hsla(from_color, to_color, delta))
                        },
                    ),
            );
        }

        let focused_id = self
            .crop_drag
            .as_ref()
            .map(|drag| drag.id)
            .or(self.hover_gate.active_item());
        // 直接借用排版里的图块，不再整份 clone；按 id 建一次索引，
        // 每块图取用的成本从 O(n) 查找降到 O(1)，整体由 O(n²) 降到 O(n)。
        let mut ordered: Vec<&CanvasEntry> = layout.entries.iter().collect();
        // 正在透视的那张最后画，幽灵层才能盖住相邻图段。
        ordered.sort_by_key(|entry| u8::from(Some(entry.id) != focused_id));
        let items: std::collections::HashMap<crate::model::ItemId, crate::model::Item> = self
            .project
            .items
            .iter()
            .map(|item| (item.id, item.clone()))
            .collect();
        for entry in ordered {
            if let Some(item) = items.get(&entry.id) {
                stack =
                    stack.child(self.render_block(theme, layout, entry, item, device_scale, cx));
            }
        }

        // 接缝只是提示：悬停或拖动作品左侧控件时一起亮起，不再自己接鼠标。
        // 间距调节和换颜色都收在左侧那一个控件里（见 `render_gap_control`）。
        let seams_lit =
            self.hover_gate.is_active(&HoverKey::widget("gap-control")) || self.gap_drag.is_some();
        for seam in &layout.seams {
            stack = stack.child(self.render_seam(theme, seam, seams_lit));
        }
        if let Some(control) = self.render_gap_control(theme, layout, cx) {
            stack = stack.child(control);
        }

        if self.dragging.is_some()
            && !self.delete_hover
            && let Some(target) = self.drag_over
        {
            // 插入线跟着排版方向走：上下堆叠画一条横线，左右排开画一条竖线。
            // 拖着图去垃圾桶时不画，免得和删除提示抢眼。
            let horizontal_flow = layout.flows_horizontally();
            let (x, y, w, h) = if horizontal_flow {
                let (x, y, height) = match target {
                    Some(id) => layout
                        .entry(id)
                        .map(|entry| {
                            (
                                entry.rect.x - layout.gap_display / 2.0 - 1.0,
                                entry.rect.y,
                                entry.rect.h,
                            )
                        })
                        .unwrap_or((0.0, 0.0, layout.height)),
                    None => (layout.width + 2.0, 0.0, layout.height),
                };
                (x, y, 2.0, height)
            } else {
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
                (x, y, width, 2.0)
            };
            stack = stack.child(
                div()
                    .debug_selector(|| "insert-line".to_string())
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(1.))
                    .bg(theme.accent),
            );
        }

        if layout.mode == Mode::Layout {
            stack = stack.child(self.render_canvas_corner(theme, layout, cx));
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
        device_scale: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let selected = self.is_selected(id);
        // 先把拖动状态里用得到的几个值取出来：后面要反复 &mut self 拿预览图，
        // 不能一直借着 self 的不可变引用。
        let drag = self
            .crop_drag
            .as_ref()
            .filter(|drag| drag.id == id)
            .map(|drag| (drag.edge, drag.frozen_origin, drag.start_band, drag.scale));
        // 拖动裁剪线时和鼠标悬停时都立刻显示被裁掉的区域，不等待。
        let focused = drag.is_some() || self.hover_gate.is_active(&HoverKey::Item(id));
        // 参与结果输出的图都能拖裁剪线：字幕模式裁上下，排版模式四条边都能裁。
        // 能进排版 entries 的图本来就参与输出，这里只需要排除还在识别的。
        let croppable = !matches!(item.status, crate::model::ItemStatus::Pending);
        let band = entry.band;
        // 拖动期间用按下时冻结的显示比例，避免每帧重算 layout 造成换算漂移。
        let (scale_x, scale_y) = match drag {
            Some((_, _, _, scale)) => (scale.x, scale.y),
            None => (
                entry.display_width / band.width().max(1) as f32,
                entry.display_height / band.height().max(1) as f32,
            ),
        };

        // 图块始终代表「可见裁剪带」：拖动时它的上/左边界跟着裁剪线走，尺寸
        // 等于可见带，不再把整张原图不透明铺开盖住后面的图片。
        let (block_left, block_top, frame_width, frame_height) = match drag {
            Some((_, origin, start_band, _)) => (
                origin.x + (band.x0 as f32 - start_band.x0 as f32) * scale_x,
                origin.y + (band.y0 as f32 - start_band.y0 as f32) * scale_y,
                band.width() as f32 * scale_x,
                band.height() as f32 * scale_y,
            ),
            None => (
                entry.rect.x,
                entry.rect.y,
                entry.display_width,
                entry.display_height,
            ),
        };
        // 整张原图在「图块」这套坐标里的左上角。始终用当前 band 反推，于是原图
        // 在屏幕上固定不动，移动的是裁剪窗口本身；位图本身不用重画。
        let full_width = item.width.max(1) as f32 * scale_x;
        let full_height = item.height.max(1) as f32 * scale_y;
        let image_left = -(band.x0 as f32 * scale_x);
        let image_top = -(band.y0 as f32 * scale_y);
        let image_width = full_width;
        let image_height = full_height;
        let visible_width = frame_width;
        let visible_height = frame_height;
        let edges = crate::app::crop_edges_for(layout.mode);
        let (tl, tr, bl, br) = block_radii(layout, entry, layout.gap_px > 0);

        // 预览位图按物理像素生成：显示宽 × 系统缩放，量化到固定档位。
        // 位图永远是整张图，裁剪靠容器裁切加位移表现，所以拖裁剪线不触发重画。
        let preview_width = preview_target_width(full_width, device_scale, item.width);
        // 拖动期间冻结预览位图：只复用已有缓存，不重新生成整图缩放。
        let freeze_preview =
            cx.has_active_drag() || self.crop_drag.is_some() || self.gap_drag.is_some();
        let preview = self.preview_for(
            id,
            item.rgb.clone(),
            preview_width,
            freeze_preview,
            cx,
        );
        // 悬停透视要一张整图的半透明底图，用的就是同一张位图。
        let full_preview = if focused && croppable {
            preview.clone()
        } else {
            None
        };

        let mut block = div()
            .debug_selector(move || format!("canvas-block-{id}"))
            .id(SharedString::from(format!("canvas-block-{id}")))
            .absolute()
            .left(px(block_left))
            .top(px(block_top))
            .w(px(frame_width))
            .h(px(frame_height))
            .cursor_pointer()
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::Item(id), *hovered, cx);
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
            .on_drag(DragItem { id }, {
                move |_value, _offset, _window, cx| cx.new(|_| CropDragGhost)
            });

        // 拖动排序的落点占位：半透明 + accent 虚线描边，和已落位的图片一眼区分。
        if entry.is_placeholder {
            block = block
                .opacity(tokens::DRAG_PLACEHOLDER_OPACITY)
                .border_1()
                .border_dashed()
                .border_color(theme.accent);
        }

        // 所有图层都以「当前裁剪带左上角」为唯一原点：裁剪线、幽灵层、
        // 磁吸刻度、图片内容共用同一套坐标，不再有两套 anchor 互相抵消。
        let ghost_anchor = ghost_crop::GhostAnchor::of_band(band);
        if let Some(full) = full_preview {
            block = block.child(hover::hover_fade(ghost_crop::render_ghost_layers(
                item,
                band,
                ghost_anchor,
                (scale_x, scale_y),
                full,
                theme,
            )));
        }

        // 可见区域始终生效：整张图按裁剪起点往负方向挪，只露出保留区。
        // 拖动时图块本身就是这个可见带，所以不再需要额外铺一张不透明整图。
        // 后台位图还没回传时先用底色占位，绝不在 UI 线程同步生成。
        let preview_layer: AnyElement = match preview {
            Some(image) => img(image)
                .absolute()
                .left(px(image_left))
                .top(px(image_top))
                .w(px(image_width))
                .h(px(image_height))
                .object_fit(ObjectFit::Fill)
                .into_any_element(),
            None => div()
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(px(visible_width))
                .h(px(visible_height))
                .bg(theme.surface)
                .into_any_element(),
        };
        block = block.child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(px(visible_width))
                .h(px(visible_height))
                .overflow_hidden()
                .rounded_tl(px(tl))
                .rounded_tr(px(tr))
                .rounded_bl(px(bl))
                .rounded_br(px(br))
                .child(preview_layer),
        );

        if selected {
            block = block.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(visible_width))
                    .h(px(visible_height))
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

        if croppable && let Some((edge, _, _, _)) = drag {
            block = block.child(ghost_crop::render_snap_ticks(
                item,
                edge,
                ghost_anchor.y0,
                frame_width,
                scale_y,
                self.project.settings.bleed,
                theme,
            ));
        }

        if croppable && focused {
            let crop_edge = drag
                .map(|(edge, _, _, _)| edge)
                .unwrap_or(crate::app::ManualEdge::Top);
            block = block.child(hover::hover_fade(ghost_crop::render_crop_lines(
                ghost_crop::CropLineView {
                    band,
                    edge: crop_edge,
                    anchor: ghost_anchor,
                    scale: (scale_x, scale_y),
                },
                theme,
                self.crop_drag
                    .as_ref()
                    .filter(|state| state.id == id)
                    .and_then(|state| state.snap),
                self.language,
            )));
        }

        if croppable {
            // 裁剪线始终贴着图块边界，热区也始终跟着边界走，不分拖动与否。
            let hotzones = [
                (ManualEdge::Top, 0.0),
                (ManualEdge::Bottom, frame_height),
                (ManualEdge::Left, 0.0),
                (ManualEdge::Right, frame_width),
            ];
            for (edge, line) in hotzones {
                if !edges.contains(&edge) {
                    continue;
                }
                block = block.child(self.render_crop_hotzone(
                    theme,
                    entry,
                    edge,
                    line,
                    (frame_width, frame_height),
                    cx,
                ));
            }
        }

        if let Some(settle) = self.settle.as_ref()
            && let Some(&(x0, y0)) = settle.from.get(&id)
        {
            let (x1, y1) = (entry.rect.x, block_top);
            if x0 != x1 || y0 != y1 {
                return block
                    .with_animation(
                        SharedString::from(format!("settle-{}-{id}", settle.generation)),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        move |el, delta| {
                            el.left(px(x0 + (x1 - x0) * delta))
                                .top(px(y0 + (y1 - y0) * delta))
                        },
                    )
                    .into_any_element();
            }
        }
        block.into_any_element()
    }

    fn render_crop_hotzone(
        &mut self,
        _theme: Theme,
        entry: &CanvasEntry,
        edge: ManualEdge,
        line: f32,
        frame: (f32, f32),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let (frame_width, frame_height) = frame;
        let thickness = f32::from(CROP_HOTZONE);
        // 接缝已经不接鼠标了（间距调节收在作品左侧的缝隙控件里），两种模式的
        // 裁剪热区都贴住裁剪线铺开，不必再往图内让出几个像素。
        let inset = 0.0;
        let name = edge.name();
        let columns = edge.is_columns();
        let (left, top, width, height, cursor) = if columns {
            let left = match edge {
                ManualEdge::Left => line + inset,
                _ => line - inset - thickness,
            };
            (
                left,
                0.0,
                thickness,
                frame_height,
                CursorStyle::ResizeLeftRight,
            )
        } else {
            let top = match edge {
                ManualEdge::Top => line + inset,
                _ => line - inset - thickness,
            };
            (0.0, top, frame_width, thickness, CursorStyle::ResizeUpDown)
        };
        div()
            .debug_selector(move || format!("crop-hotzone-{id}-{name}"))
            .id(SharedString::from(format!("crop-hotzone-{id}-{name}")))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .cursor(cursor)
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

    /// 接缝的那道光带。
    ///
    /// 接缝不再接收鼠标，所以这里没有指针监听，也没有光标：间距和颜色都收在
    /// 作品左侧的缝隙控件里。`lit` 为真（指针压着控件、或正在拖间距）时，
    /// 一条 2 像素的亮线在光带中间亮起，用来指出这一拖会改哪些接缝。
    fn render_seam(&mut self, theme: Theme, seam: &Seam, lit: bool) -> AnyElement {
        let index = seam.index;
        let seam_el = div()
            .debug_selector(move || format!("seam-{index}"))
            .id(SharedString::from(format!("seam-{index}")))
            .absolute()
            .left(px(seam.rect.x))
            .top(px(seam.rect.y))
            .w(px(seam.rect.w))
            .h(px(seam.rect.h))
            .when(lit, |this| {
                this.child(
                    div()
                        .debug_selector(move || format!("seam-lit-{index}"))
                        .absolute()
                        .left(px(0.))
                        .right(px(0.))
                        .top(px((seam.rect.h - 2.0) / 2.0))
                        .h(px(2.))
                        .bg(theme.accent.opacity(0.55)),
                )
            });
        if let Some(settle) = self.settle.as_ref()
            && let Some(&(x0, y0)) = settle.from_seams.get(&index)
        {
            let (x1, y1) = (seam.rect.x, seam.rect.y);
            if x0 != x1 || y0 != y1 {
                return seam_el
                    .with_animation(
                        SharedString::from(format!("settle-seam-{}-{index}", settle.generation)),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        move |el, delta| {
                            el.left(px(x0 + (x1 - x0) * delta))
                                .top(px(y0 + (y1 - y0) * delta))
                        },
                    )
                    .into_any_element();
            }
        }
        seam_el.into_any_element()
    }

    /// 缩放胶囊：显示作品在屏幕上的实际比例，点一下回到 100%。
    fn render_zoom_chip(
        &mut self,
        theme: Theme,
        display_scale: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = self.language;
        // 显示自适应比例和用户缩放叠起来的最终比例，不写用户缩放值，
        // 否则「100%」和大图被压到 80% 的真实画面会对不上。
        let percent = (display_scale * 100.0).round() as u32;
        let fade = self.hover_gate.fade_delta(&HoverKey::widget("zoom-chip"));
        let chip = div()
            .debug_selector(|| "zoom-chip".to_string())
            .id("zoom-chip")
            .px(px(10.))
            .py(px(5.))
            .rounded(radius::PILL)
            .bg(hover::fade_background(
                Some(theme.surface),
                theme.surface_elevated,
                fade,
            ))
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(theme.muted)
            .cursor_pointer()
            .on_hover(
                cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("zoom-chip"), *hovered, cx);
                }),
            )
            .on_click(
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.action_zoom_reset(cx);
                }),
            )
            // 缩放胶囊在画布里面，按下时不要让画布把它当成“空白处拖窗口”。
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(format!(
                "{} · {}",
                language.zoom_percent(percent),
                language.zoom_reset_hint()
            ));
        let wrapper = div().absolute().right(px(20.)).bottom(px(20.)).child(chip);
        wrapper.with_animation(
            "zoom-chip-enter",
            Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
            |el, delta| el.opacity(delta).bottom(px(20.0 + 4.0 * (1.0 - delta))),
        )
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

/// 拖动图片时跟着指针走的小图，画在窗口坐标里。
///
/// 以前交给 GPUI 的“拖动视图”去画，那个视图会被单独预排版，尺寸、裁剪和
/// 绘制顺序任一处不对就看不见。现在改成根节点的普通子元素，位置由
/// `drag_thumb_rect` 算出来，画在所有界面内容之上。
pub(crate) fn drag_thumb_rect(
    pointer: gpui::Point<gpui::Pixels>,
    source_width: u32,
    source_height: u32,
) -> Rect {
    let (width, height) = drag_thumb_size(source_width, source_height);
    Rect::new(
        f32::from(pointer.x) - width / 2.0,
        f32::from(pointer.y) - height / 2.0,
        width,
        height,
    )
}

impl AppRoot {
    /// 拖动图片时的半透明小图。没有拖动或还没收到指针位置时不画。
    pub(crate) fn render_drag_thumb(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let id = self.dragging?;
        let pointer = self.drag_pointer?;
        let (source, source_width, source_height) = {
            let item = self.project.items.iter().find(|item| item.id == id)?;
            (item.rgb.clone(), item.width, item.height)
        };
        let rect = drag_thumb_rect(pointer, source_width, source_height);
        let target_width = preview_target_width(rect.w, window.scale_factor(), source_width);
        // 拖动期间冻结预览：优先复用这张图已有的位图，没有才回传一次。
        let image = self.preview_for(id, source, target_width, true, cx);
        let layer: AnyElement = match image {
            Some(image) => img(image).w_full().h_full().object_fit(ObjectFit::Fill).into_any_element(),
            None => div().w_full().h_full().bg(theme.surface).into_any_element(),
        };
        Some(
            div()
                .debug_selector(|| "drag-thumb".to_string())
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.w))
                .h(px(rect.h))
                .rounded(radius::SMALL)
                .overflow_hidden()
                .opacity(tokens::DRAG_THUMB_OPACITY)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border_subtle)
                .shadow(theme.shadow_float())
                .child(layer)
                .into_any_element(),
        )
    }
}

/// 拖动小图的尺寸：最长边固定 `DRAG_THUMB_SIDE`，另一边按原图宽高比换算。
fn drag_thumb_size(source_width: u32, source_height: u32) -> (f32, f32) {
    let width = source_width.max(1) as f32;
    let height = source_height.max(1) as f32;
    let side = tokens::DRAG_THUMB_SIDE;
    if width >= height {
        (side, (side * height / width).max(1.0))
    } else {
        ((side * width / height).max(1.0), side)
    }
}

/// 预览宽度量化档位（位图像素）。
///
/// 同一档位内的显示宽变化复用同一位图，窗口缩放和画布缩放时
/// 不会每帧都重新生成预览；向上量化意味着位图只比物理网格大一点，
/// 渲染时轻微缩小采样，不会重新发糊。
pub(crate) const PREVIEW_BUCKET: u32 = 256;

/// 预览位图的目标宽度（位图像素）。
///
/// GPU 渲染时按物理像素采样：位图若只按逻辑像素生成、或被上限截断得
/// 比物理像素网格还小，画面就会被拉伸采样而发糊。所以宽度按
/// “显示宽 × 系统缩放”计算并向上量化到档位；源图不够宽时上限就是
/// 源图宽——放大源图不会增加任何细节。
pub(crate) fn preview_target_width(display_width: f32, scale: f32, source_width: u32) -> u32 {
    let device_width = (display_width.max(0.0) * scale.max(1.0)).ceil() as u32;
    let bucketed = device_width.div_ceil(PREVIEW_BUCKET) * PREVIEW_BUCKET;
    bucketed
        .clamp(PREVIEW_MIN_WIDTH, PREVIEW_MAX_WIDTH)
        .min(source_width.max(1))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_width_follows_device_pixels() {
        // 1× 缩放：1000 逻辑宽向上落到 1024 档
        assert_eq!(preview_target_width(1000.0, 1.0, 3840), 1024);
        // 150% 缩放：物理需求 1500，落到 1536 档
        assert_eq!(preview_target_width(1000.0, 1.5, 3840), 1536);
        // 200% 缩放：位图按物理像素翻倍，不再按逻辑像素截断
        assert_eq!(preview_target_width(1000.0, 2.0, 3840), 2048);
    }

    #[test]
    fn preview_width_respects_source_and_caps() {
        // 绝对上限压住极端需求，控制单张内存
        assert_eq!(preview_target_width(4000.0, 1.0, 8000), PREVIEW_MAX_WIDTH);
        // 源图不够宽时不超过源图：放大源图不会增加细节
        assert_eq!(preview_target_width(4000.0, 1.0, 1920), 1920);
        // 很小的显示宽保留下限，超采样保证小图清晰
        assert_eq!(preview_target_width(50.0, 1.0, 4000), PREVIEW_MIN_WIDTH);
    }

    #[test]
    fn drag_thumb_is_centered_on_pointer() {
        let rect = drag_thumb_rect(gpui::point(px(400.), px(300.)), 1600, 900);
        // 横图：最长边 88，高按比例，中心正好落在指针上。
        assert!((rect.w - tokens::DRAG_THUMB_SIDE).abs() < 1e-3);
        assert!((rect.h - 88.0 * 900.0 / 1600.0).abs() < 1e-3);
        assert!((rect.x + rect.w / 2.0 - 400.0).abs() < 1e-3);
        assert!((rect.y + rect.h / 2.0 - 300.0).abs() < 1e-3);

        // 竖图：最长边是高度。
        let rect = drag_thumb_rect(gpui::point(px(10.), px(20.)), 900, 1600);
        assert!((rect.h - tokens::DRAG_THUMB_SIDE).abs() < 1e-3);
        assert!((rect.w - 88.0 * 900.0 / 1600.0).abs() < 1e-3);
    }
}
