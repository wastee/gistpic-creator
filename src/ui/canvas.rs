//! 画布：字幕模式与排版模式的实时预览，以及画布内的直接手势。
//!
//! 画布上没有常驻按钮：悬停看透视、拉裁剪线、拖接缝、拉框多选，
//! 所有操作都在图片和留白上完成。

mod gap_control;
mod geometry;
mod navigator;

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
pub(crate) use navigator::{navigator_rect, scroll_for_frame, thumbnail_bounds, viewport_frame};

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
        //
        // 裁剪拖动期间还要撑住按下时的滚动范围：裁剪让作品变小之后，滚动区的
        // 内容跟着变矮，GPUI 重排时会把滚动偏移夹回新范围，画面被一股一股地拽
        // 向作品开头。撑住内容尺寸，夹紧就不会发生。
        let (floor_w, floor_h) = layout.scroll_content_floor(viewport);
        let content_min_width =
            px((layout.width + 2.0 * f32::from(CANVAS_SIDE_PAD)).max(floor_w));
        let root = div()
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
            // 需求 6：按住中键拖动可以平移画布显示范围。三个监听只负责把事件
            // 接到状态层的处理函数上，真正的平移逻辑在 canvas_gestures.rs 里。
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::canvas_middle_down))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::canvas_middle_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::canvas_middle_up))
            // 中键平移期间光标是握紧的手；安静时画布空白处保持箭头（左键拖窗口）。
            .when(self.is_middle_panning(), |this| {
                this.cursor(CursorStyle::ClosedHand)
            })
            .on_mouse_move(cx.listener(Self::canvas_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::canvas_right_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::canvas_right_up_out))
            .child(
                div()
                    .w_full()
                    .min_w(content_min_width)
                    .min_h(px(floor_h))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(body),
            );

        // 缩放胶囊不挂在这里：它是窗口级浮层，见 `AppRoot::render`。
        // 挂在滚动区里会跟着作品一起滚走，作品一长三个点击项就够不着了。
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
                    // 需求 1：拖起来之后光标要变成「握紧的手」。
                    //
                    // gpui 在拖动启动那一刻就把源元素当时的光标冻结进拖动记录，
                    // 之后每帧由拖动记录接管，元素上的 .cursor(...) 不再起作用。
                    // 只有 App::set_active_drag_cursor_style 能改这份冻结的值。
                    if cx.active_drag_cursor_style() != Some(CursorStyle::ClosedHand) {
                        cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    }
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

        stack
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
        // 需求 3：半透明预览只在真的拖着这张图的裁剪边时浮出来。
        //
        // 指针停在有裁切的图上时，只留四条细线告诉用户「这张图可以裁」；浮出
        // 整张图的预览会被误以为是「已经完整显示」，而且挡住底下的图。
        let full_preview = if drag.is_some() && croppable {
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
            // 需求 1：图片块可以按住拖动排序，悬停用「张开的手」表示可以抓手。
            .cursor(CursorStyle::OpenHand)
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
            // 需求 8：被作用的那张图整圈描一条细亮边，一眼看出现在动的是哪一张。
            // 亮边的强度跟着边缘提示的进度走：指针刚碰上时很淡，停稳或拖动时更清楚。
            let strength = self
                .edge_hint_opacity_at(id, cx.background_executor().now())
                .clamp(tokens::CROP_HINT_REST, 1.0);
            block = block.child(
                div()
                    .debug_selector(move || format!("block-focus-{id}"))
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(visible_width))
                    .h(px(visible_height))
                    .rounded_tl(px(tl))
                    .rounded_tr(px(tr))
                    .rounded_bl(px(bl))
                    .rounded_br(px(br))
                    .border_1()
                    .border_color(theme.accent.opacity(0.25 + 0.40 * strength)),
            );
            block = block.child(ghost_crop::render_crop_lines(
                ghost_crop::CropLineView {
                    band,
                    // 只有真的在拖这张图时才突出某一条边。指针只是停在图块上
                    // 的时候四条边一视同仁，免得看起来像已经在调某条边。
                    dragging: drag.map(|(edge, _, _, _)| edge),
                    anchor: ghost_anchor,
                    scale: (scale_x, scale_y),
                },
                theme,
                self.crop_drag
                    .as_ref()
                    .filter(|state| state.id == id)
                    .and_then(|state| state.snap),
                self.language,
                // 需求 10：边缘提示按时间显现。指针靠近的过程由 `update_edge_hint`
                // 推进，拖动中停在满格，松手后退回默认的半透明。
                self.edge_hint_opacity_at(id, cx.background_executor().now()),
            ));
        }

        if croppable {
            // 裁剪线始终贴着图块边界，热区也始终跟着边界走，不分拖动与否。
            let hotzones = [
                // 需求 4：窄图四边的热区要能分别按到，就不能让相邻两条热区
                // 在角上互相盖住。原尺寸太小时按图块尺寸收缩，四边各占一段。
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
                    layout.edge_touches_neighbour(entry, edge),
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

    /// 一条裁剪边的热区。`seam_edge` 表示这条边紧贴着另一张图
    /// （由 `CanvasLayout::edge_touches_neighbour` 判定），只有这种边才往图内
    /// 让出死区。
    fn render_crop_hotzone(
        &mut self,
        _theme: Theme,
        entry: &CanvasEntry,
        edge: ManualEdge,
        line: f32,
        frame: (f32, f32),
        seam_edge: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let (frame_width, frame_height) = frame;
        let thickness = f32::from(CROP_HOTZONE);
        // 需求 8：两张图紧贴（间距为 0）时，接缝两侧各留 3 像素不接鼠标，合起来
        // 就是 SEAM_DEADZONE。这样按在接缝附近时不会误按到隔壁那张图的边——
        // 上下相邻时就是「想调第二张的顶边，结果调到了第一张的底边」。
        //
        // 只对真的有紧贴邻居的边让位：外沿（第一张的顶边、最后一张的底边、单张图
        // 的四条边）没有歧义，间距大于 0 时两块图之间隔着可见缝隙、热区本来就
        // 不会重叠，这两种情况都照旧贴住裁剪线，不做无谓的缩进。
        let inset = if seam_edge {
            f32::from(tokens::SEAM_DEADZONE) / 2.0
        } else {
            0.0
        };
        let name = edge.name();
        let columns = edge.is_columns();
        // 需求 4/5：热区既要在四角互不遮挡，又要在很窄、很矮的图块上按得到。
        //
        // 把图块按宽高各三等分来看：上下两条横热区各占最上、最下一档（横向铺满
        // 整个图块），左右两条竖热区放在中间那一档（纵向占满中间档）。这样：
        //
        // - 横竖两组永远不会在角上重叠，按左边只会拿到左边，不会拿到上边；
        // - 每条热区的外沿都正好压在图块边上，视觉上的裁剪线在哪儿，热区就在哪儿；
        // - 图块被裁到很小时，每一档跟着变小但依然存在，四个方向都还有地方按，
        //   不会退化成按不到的一条缝。
        //
        // 为什么是三等分而不是「最多 16 像素」：图块只剩几个像素时，16 像素的
        // 热区会横跨整个图块、盖住对面那条边，按右上角会变成按上边。取三分之一
        // 就保证了同一轴上两条热区之间永远隔着中间那一档。
        let band_x = thickness.min(frame_width / 3.0).max(0.0);
        let band_y = thickness.min(frame_height / 3.0).max(0.0);
        let (left, top, width, height, cursor) = if columns {
            let left = match edge {
                ManualEdge::Left => line + inset,
                _ => line - inset - band_x,
            };
            (
                left,
                band_y,
                band_x,
                (frame_height - 2.0 * band_y).max(0.0),
                CursorStyle::ResizeLeftRight,
            )
        } else {
            let top = match edge {
                ManualEdge::Top => line + inset,
                _ => line - inset - band_y,
            };
            // 上下两条横向铺满整个图块，不左右缩进。四角因此归上下边，
            // 图块上不会留下「按了没反应」的空档；左右两条退到中间那一档，
            // 让开四角，也就不会出现「按右边拿到上边」。
            (0.0, top, frame_width, band_y, CursorStyle::ResizeUpDown)
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
                        // 需求 4/5：裁剪边按下就掐断冒泡。
                        //
                        // 热区画在图块里，图块下面是第 2 张图的图块、再下面是画布。
                        // 不掐断的话，上面这张的处理函数先跑，但它拿不到有效裁剪带时
                        // 会直接返回，事件继续往下走：下一条热区、或者画布的空白处
                        // 处理函数接着跑一遍。结果是「拖这张图的边，动的却是另一张」
                        // 甚至「变成拖动整个窗口」。这里按下去就先截住，再开始拖动。
                        this.begin_crop_drag(CropHandleDrag { id, edge }, event, window, cx);
                        cx.stop_propagation();
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

    /// 缩放胶囊：显示作品在屏幕上的真实比例，并提供「适合画布」和「1:1」。
    ///
    /// 百分比是「显示像素 / 成品输出像素」的真实比值，不写用户的相对缩放值：
    /// 一张 1920 宽的图被窗口压到一半时读数是 50%，不会读成 100%。
    /// 「适合画布」和「1:1」是两个各自独立、各自可点击的项，读数本身不接点击，
    /// 也不做可点的样子。胶囊常驻显示，随时能看出当前比例、随时能切换。
    pub(crate) fn render_zoom_chip(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let layout = self.canvas_layout();
        let view = layout.view;
        let display_scale = layout.scale;
        let language = self.language;
        let percent = (display_scale * 100.0).round() as u32;
        // 用排版自己记的模式判断哪一项是当前项，和上面那个百分比出自同一份数据。
        let fit_active = view.is_fit();
        // 需求 4：当前项蓝底白字，另一项灰字无底色，跟胶囊里排版预设的写法一致。
        // 读数不是当前项：用中性色，指针压上去才提亮，别让人以为点了会怎样。
        let reading = self.zoom_chip_part(
            "zoom-reading",
            language.zoom_percent(percent),
            if self.hover_gate.is_active(&HoverKey::widget("zoom-reading")) {
                theme.text
            } else {
                theme.muted
            },
            false,
            false,
            theme,
            cx,
        );
        let fit = self.zoom_chip_part(
            "zoom-fit",
            language.zoom_fit(),
            theme.muted,
            fit_active,
            true,
            theme,
            cx,
        );
        let one_to_one = self.zoom_chip_part(
            "zoom-one-to-one",
            language.zoom_one_to_one(),
            theme.muted,
            !fit_active,
            true,
            theme,
            cx,
        );
        let chip = div()
            .debug_selector(|| "zoom-chip".to_string())
            .id("zoom-chip")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .px(px(5.))
            .py(px(3.))
            .rounded(radius::PILL)
            .bg(theme.surface.opacity(0.94))
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            // 胶囊浮在画布上，按下时别让画布把它当成“空白处拖窗口”。
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(reading)
            .child(div().w(px(1.)).h(px(12.)).bg(theme.border_subtle))
            .child(fit)
            .child(one_to_one);
        // 这块浮层挂在滚动区外面，用窗口右下角定位，不跟着作品滚。
        Some(
            div()
                .absolute()
                .right(px(20.))
                .bottom(px(20.))
                .child(chip.with_animation(
                    "zoom-chip-enter",
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                ))
                .into_any_element(),
        )
    }

    /// 缩放胶囊里的一项。`active` 为真表示这是当前生效的那一项：蓝底白字；
    /// 不然就用 `color`（另一项和读数都传灰字）加悬停色。`clickable` 为真时
    /// 接点击，并在悬停时补一句说明。
    fn zoom_chip_part(
        &mut self,
        id: &'static str,
        label: impl Into<SharedString>,
        color: gpui::Hsla,
        active: bool,
        clickable: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = label.into();
        let key = HoverKey::widget(id);
        let fade = self.hover_gate.fade_delta(&key);
        let hint =
            self.render_attached_hint(&key, crate::ui::hint::HINT_GAP + px(20.), theme);
        let mut part = div()
            .debug_selector(move || format!("zoom-item-{id}"))
            .id(id)
            .px(px(6.))
            .py(px(2.))
            .rounded(radius::SMALL)
            .bg(hover::fade_background(
                active.then_some(theme.accent),
                theme.hover,
                fade,
            ))
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(if active { theme.accent_text } else { color })
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(key.clone(), *hovered, cx);
                }),
            );
        if clickable {
            part = part.cursor_pointer().on_click(cx.listener(
                move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    if id == "zoom-fit" {
                        this.action_zoom_fit(cx);
                    } else {
                        this.action_zoom_one_to_one(cx);
                    }
                },
            ));
        }
        // 需求 4：当前项再挂一个标记，只有处于选中状态的那一项才有这个选择器。
        // 颜色在无头测试里读不到，测试靠它判断「现在生效的是哪一项」，以及这一项
        // 在胶囊里的位置。标记挂在最外层，它的范围就等于这一项自己的范围。
        let short = id.strip_prefix("zoom-").unwrap_or(id);
        let mut wrapper = div()
            .relative()
            .when(active, |this| {
                this.debug_selector(move || format!("zoom-active-{short}"))
            })
            .child(part.child(label));
        if let Some(hint) = hint {
            wrapper = wrapper.child(hint);
        }
        wrapper.into_any_element()
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

    /// 需求 4：缩放胶囊里只有当前生效的那一项带 `zoom-active-{id}` 标记，而且
    /// 标记的范围就等于这一项自己的范围，测试才能靠它读出「现在选的是哪一项」。
    ///
    /// 为什么分两个窗口：`debug_bounds` 读的是渲染帧里一张只增不减的表，
    /// 在一个窗口里断言某个选择器「不在了」永远会失败（见 `src/app/ui_smoke.rs`
    /// 里同样的说明）。而 `add_window_view` 开窗时就已经画了第一帧，那时永远
    /// 是「适合画布」，所以 `zoom-active-fit` 在每个窗口里都必然出现过，它「不在」
    /// 这件事任何窗口都断言不出来。能稳定断言的只有「还没切过去的那一项不在」：
    /// 第一帧只有 fit 那一项有标记，所以 `zoom-active-one-to-one` 在切过去之前
    /// 确实不在。
    #[test]
    fn zoom_active_marker_marks_only_the_current_item() {
        use gpui::{AppContext, TestAppContext};

        // 窗口一：停在默认的「适合画布」。
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(crate::app::AppRoot::new);
        install_zoom_test_item(cx, &view);
        draw_zoom_test(cx);
        assert!(
            cx.read_entity(&view, |root, _| root.view_scale().is_fit()),
            "默认应是适合画布"
        );
        let fit_item = cx
            .debug_bounds("zoom-item-zoom-fit")
            .expect("应有适合画布项");
        assert_eq!(
            cx.debug_bounds("zoom-active-fit"),
            Some(fit_item),
            "适合画布是当前项，标记的范围应等于这一项自己的范围"
        );
        assert!(
            cx.debug_bounds("zoom-active-one-to-one").is_none(),
            "还没切到 1:1，不该有它的标记"
        );

        // 窗口二：切到 1:1，标记应落到 1:1 那一项上。
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(crate::app::AppRoot::new);
        install_zoom_test_item(cx, &view);
        cx.update_entity(&view, |root, cx| {
            root.action_zoom_one_to_one(cx);
        });
        draw_zoom_test(cx);
        assert!(
            !cx.read_entity(&view, |root, _| root.view_scale().is_fit()),
            "点过 1:1 之后不该还是自动适应"
        );
        let one_item = cx
            .debug_bounds("zoom-item-zoom-one-to-one")
            .expect("应有 1:1 项");
        assert_eq!(
            cx.debug_bounds("zoom-active-one-to-one"),
            Some(one_item),
            "1:1 是当前项，标记的范围应等于这一项自己的范围"
        );
        // 标记真的跟着走了：1:1 那一项和适合画布不是同一块地方。
        let fit_item = cx
            .debug_bounds("zoom-item-zoom-fit")
            .expect("应有适合画布项");
        assert!(
            one_item.origin.x > fit_item.origin.x,
            "1:1 项应在适合画布项右边，实际 {:?} 对 {:?}",
            one_item.origin,
            fit_item.origin
        );

        // 悬停会往这一项上面挂说明气泡。气泡是绝对定位、不占位置，标记的范围
        // 不该因此变大或挪位，测试读到的位置才一直是这一项自己的位置。
        //
        // 先把指针放到左上角、走进图块，再走回胶囊：命中结果读的是上一帧的
        // 位置，不先把指针挪开一步，第一下移动未必立刻改成胶囊上的目标。
        cx.simulate_mouse_move(gpui::point(px(2.), px(2.)), None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw_zoom_test(cx);
        cx.simulate_mouse_move(one_item.center(), None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw_zoom_test(cx);
        assert_eq!(
            cx.read_entity(&view, |root, _| root.hover_gate.debug_active()),
            Some(HoverKey::widget("zoom-one-to-one")),
            "指针压到 1:1 上时应把悬停目标设成它"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(
                crate::ui::hint::HINT_ENTER_MS + 20,
            ));
        cx.run_until_parked();
        draw_zoom_test(cx);
        assert!(
            cx.debug_bounds("hover-hint-zoom-one-to-one").is_some(),
            "1:1 处于悬停时应画出它的说明气泡"
        );
        assert_eq!(
            cx.debug_bounds("zoom-active-one-to-one"),
            Some(one_item),
            "挂了说明气泡之后，标记的范围仍应等于这一项自己的范围"
        );
    }

    /// 给缩放胶囊的测试铺一张必然被压到 100% 以下的大图。这里只铺数据不画帧，
    /// 由调用方决定什么时候画第一帧（先画哪一帧，就决定了哪一项会进 debug_bounds）。
    fn install_zoom_test_item(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>) {
        use gpui::AppContext;
        cx.update_entity(view, |root, cx| {
            root.project.items = vec![crate::model::Item {
                id: 1,
                name: "测试图1.png".into(),
                width: 3200,
                height: 2400,
                rgb: std::sync::Arc::new(image::RgbImage::from_pixel(
                    3200,
                    2400,
                    image::Rgb([40, 40, 44]),
                )),
                block: None,
                status: crate::model::ItemStatus::NoSubtitle,
                keep_whole: true,
                manual: None,
                analysis: Default::default(),
                fingerprint: Default::default(),
            }];
            root.selected = Some(1);
            root.selected_ids = [1].into_iter().collect();
            cx.notify();
        });
    }

    /// 画一帧并等布局稳定，让 `debug_bounds` 能读到最新的位置。
    fn draw_zoom_test(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
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
