//! 画布手势与右键卡片的交互逻辑。
//!
//! 这一层只改应用状态；画布怎么画在 ui/canvas.rs，状态字段在 app.rs。

use gpui::{
    Context, DragMoveEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ScrollDelta,
    ScrollWheelEvent, Window, point,
};

use crate::app::crop_drag;
use crate::app::{
    AppRoot, BleedDrag, CanvasCornerDrag, CropDragState, CropHandleDrag, GapDrag, GapDragState,
    ManualEdge, MarqueeDrag, MenuKind, MenuState, RightPress,
};
use crate::model::{GapSetting, ItemId, ManualBand};
use crate::ui::canvas::{CanvasLayout, DragPreview, Rect, auto_scroll_intensity, marquee_hits};
use crate::ui::ghost_crop;
use crate::ui::tokens;

/// 视口尺寸换成纯 `f32`，几何计算不关心 `Pixels` 包装。
fn viewport_f32(viewport: gpui::Size<gpui::Pixels>) -> gpui::Size<f32> {
    gpui::size(f32::from(viewport.width), f32::from(viewport.height))
}

impl AppRoot {
    /// 构造画布排版，并在裁剪拖动期间冻结上方留白。
    ///
    /// 所有需要排版的地方都走这里，保证渲染与坐标换算看到的是同一份原点。
    pub(crate) fn build_canvas_layout(
        &self,
        preview: DragPreview,
        viewport: gpui::Size<gpui::Pixels>,
    ) -> CanvasLayout {
        let layout = CanvasLayout::new(&self.project, self.zoom, preview, viewport);
        match self.crop_drag.as_ref() {
            Some(drag) => layout.freeze_top_padding(drag.frozen_top_padding),
            None => layout,
        }
    }

    /// 当前画布的排版。拖动排序时被拖的那张按临时顺序显示成半透明占位，
    /// 其余图片即时让位，松手前就能看到 1 3 2 这样的落点。
    pub(crate) fn canvas_layout(&self) -> CanvasLayout {
        let preview = match self.dragging {
            Some(id) => DragPreview::Placeholder {
                id,
                before: self.drag_over.flatten(),
            },
            None => DragPreview::None,
        };
        self.build_canvas_layout(preview, self.viewport)
    }

    /// 把窗口坐标换成作品内容坐标。
    pub(crate) fn content_position(
        &self,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
    ) -> gpui::Point<f32> {
        self.content_position_excluding(position, window, self.dragging)
    }

    /// 同上，但可以指定排版时先摘掉哪一张（拖动排序时用被拖的那张）。
    ///
    /// 显式传 `hidden`，拖动刚开始、`self.dragging` 还没落下的那一帧也能按合拢后的
    /// 排版换算坐标，落点和插入线不会先闪一下。
    pub(crate) fn content_position_excluding(
        &self,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
        hidden: Option<ItemId>,
    ) -> gpui::Point<f32> {
        self.content_position_in(position, window.viewport_size(), hidden)
    }

    /// 同上，但只依赖应用里记着的视口大小，不要求调用方拿着 `Window`。
    /// 后台计时器（贴边自动滚动）里靠它把指针换算成作品坐标。
    pub(crate) fn content_position_in(
        &self,
        position: gpui::Point<gpui::Pixels>,
        viewport: gpui::Size<gpui::Pixels>,
        hidden: Option<ItemId>,
    ) -> gpui::Point<f32> {
        let preview = match hidden {
            Some(id) => DragPreview::Hidden(id),
            None => DragPreview::None,
        };
        let layout = self.build_canvas_layout(preview, viewport);
        let origin = layout.origin(viewport, self.canvas_scroll.offset());
        point(
            f32::from(position.x) - origin.x,
            f32::from(position.y) - origin.y,
        )
    }

    // ------------------------------------------------------------------
    // 悬停
    // ------------------------------------------------------------------

    // 悬停上报统一走 ui::hover 里的 `set_hover`，画布手势层不再自己管悬停状态。

    // ------------------------------------------------------------------
    // 裁剪线
    // ------------------------------------------------------------------

    pub(crate) fn begin_crop_drag(
        &mut self,
        drag: CropHandleDrag,
        event: &MouseDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let layout = self.canvas_layout();
        let Some(entry) = layout.entry(drag.id).copied() else {
            return;
        };
        let Some(item) = self.project.items.iter().find(|item| item.id == drag.id) else {
            return;
        };
        let image_size = (item.width, item.height);
        let Some(start) = self.current_manual_for(drag.id) else {
            return;
        };
        if !self.selected_ids.contains(&drag.id) {
            self.select(drag.id, cx);
        }
        let targets = self.crop_drag_targets(drag.id, drag.edge);
        let band = entry.band;
        // 冻结按下时的显示比例、图块原点与上方留白：拖动全程用同一套坐标，
        // 不让每帧重算的 layout 把裁剪线、图片内容和留白各推一次。
        let scale = point(
            entry.display_width / band.width().max(1) as f32,
            entry.display_height / band.height().max(1) as f32,
        );
        let frozen_top_padding = layout.top_padding(window.viewport_size());
        self.crop_drag = Some(CropDragState {
            id: drag.id,
            edge: drag.edge,
            start,
            targets,
            start_pointer: point(f32::from(event.position.x), f32::from(event.position.y)),
            frozen_origin: entry.rect.origin(),
            start_band: band,
            frozen_top_padding,
            auto_scroll_pixels: point(0.0, 0.0),
            image_size,
            scale,
            undo_recorded: false,
            snap: None,
        });
        cx.notify();
    }

    pub(crate) fn drag_crop_handle(
        &mut self,
        drag: CropHandleDrag,
        event: &DragMoveEvent<CropHandleDrag>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.crop_drag.as_ref() else {
            return;
        };
        if state.id != drag.id || state.edge != drag.edge {
            return;
        }
        let edge = state.edge;
        let start = state.start;
        let image_size = state.image_size;
        let scale = state.scale;
        let start_pointer = state.start_pointer;
        let recorded_auto_scroll = state.auto_scroll_pixels;
        let targets = state.targets.clone();
        let viewport = window.viewport_size();
        let pointer = event.event.position;
        let delta = point(
            f32::from(pointer.x) - start_pointer.x,
            f32::from(pointer.y) - start_pointer.y,
        );

        // 靠近视口边缘时自动滚动，滚轮位移要从拖动量里扣掉，
        // 这样指针不动、画面在动的时候裁剪线也不会跟着漂。
        let mut scroll_step = point(gpui::px(0.), gpui::px(0.));
        if edge.is_columns() {
            if delta.x < 0.0 && pointer.x < gpui::px(96.) {
                scroll_step.x = gpui::px(12.);
            } else if delta.x > 0.0 && pointer.x > viewport.width - gpui::px(96.) {
                scroll_step.x = -gpui::px(12.);
            }
        } else if delta.y < 0.0 && pointer.y < gpui::px(96.) {
            scroll_step.y = gpui::px(12.);
        } else if delta.y > 0.0 && pointer.y > viewport.height - gpui::px(96.) {
            scroll_step.y = -gpui::px(12.);
        }
        if scroll_step != point(gpui::px(0.), gpui::px(0.)) {
            let scroll = self.canvas_scroll.offset();
            let max_scroll = self.canvas_scroll.max_offset();
            let next = point(
                (scroll.x + scroll_step.x).clamp(-max_scroll.width, gpui::px(0.)),
                (scroll.y + scroll_step.y).clamp(-max_scroll.height, gpui::px(0.)),
            );
            self.canvas_scroll.set_offset(next);
            if let Some(state) = self.crop_drag.as_mut() {
                state.auto_scroll_pixels.x += f32::from(scroll_step.x);
                state.auto_scroll_pixels.y += f32::from(scroll_step.y);
            }
        }
        let auto_scroll = point(
            recorded_auto_scroll.x + f32::from(scroll_step.x),
            recorded_auto_scroll.y + f32::from(scroll_step.y),
        );

        // 上下边按原图高度换算，左右边按原图宽度换算。
        let (drag_px, image_px, axis_scale) = if edge.is_columns() {
            (delta.x - auto_scroll.x, image_size.0 as f32, scale.x)
        } else {
            (delta.y - auto_scroll.y, image_size.1 as f32, scale.y)
        };
        let delta_ratio = drag_px / (image_px * axis_scale).max(1.0);

        let Some(item) = self.project.items.iter().find(|item| item.id == drag.id) else {
            return;
        };
        // 只有上下边有吸附档位（字幕块给的是上下边界），左右边自由拖动。
        let snapped = if edge.is_columns() {
            None
        } else {
            Some(ghost_crop::snap_crop_ratio_with_display(
                item,
                edge,
                match edge {
                    ManualEdge::Top => start.top,
                    _ => start.bottom,
                } + delta_ratio,
                self.project.settings.bleed,
                image_size.1 as f32 * scale.y,
            ))
        };
        if let Some(snapped) = snapped.as_ref()
            && let Some(state) = self.crop_drag.as_mut()
        {
            state.snap = snapped.tier;
        }
        let effective_delta = match snapped.as_ref() {
            Some(snapped) => {
                let raw = match edge {
                    ManualEdge::Top => start.top,
                    _ => start.bottom,
                };
                snapped.ratio - raw
            }
            None => delta_ratio,
        };

        let bands = crop_drag::drag_bands(&targets, edge, effective_delta);
        if bands.is_empty() {
            return;
        }
        // 已经贴着图片边缘，继续往外拖不产生任何变化，也不记撤销。
        if bands.iter().all(|(id, next)| {
            self.project
                .items
                .iter()
                .find(|item| item.id == *id)
                .and_then(|item| item.manual)
                == Some(*next)
        }) {
            return;
        }
        if !self
            .crop_drag
            .as_ref()
            .is_some_and(|state| state.undo_recorded)
        {
            self.record_undo();
            if let Some(state) = self.crop_drag.as_mut() {
                state.undo_recorded = true;
            }
        }
        for (id, next) in bands {
            if let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) {
                item.manual = Some(next);
                item.keep_whole = false;
            }
        }
        // 预览位图只跟图片和显示宽有关，裁剪范围是靠界面裁切加位图位移做出来的，
        // 所以这里改裁剪范围不用清缓存——清了反而每拖一步都重画位图，拖尺寸就卡。
        cx.notify();
    }

    /// 这一拖要跟着改哪些图。
    ///
    /// 横排共用高度、竖排共用宽度，改共用那条边时整组一起改；其余情况只改被拖的那张
    /// （多选时改所有选中的图）。字幕模式一条一条改，多选时同步。
    fn crop_drag_targets(&self, dragged: ItemId, edge: ManualEdge) -> Vec<(ItemId, ManualBand)> {
        let group =
            crop_drag::group_follows(self.project.mode, self.project.settings.layout, edge.axis());
        let mut targets: Vec<ItemId> = if group {
            self.project.items.iter().map(|item| item.id).collect()
        } else if self.selected_ids.len() > 1 {
            self.selected_ids.iter().copied().collect()
        } else {
            vec![dragged]
        };
        if !targets.contains(&dragged) {
            targets.push(dragged);
        }
        targets
            .into_iter()
            .filter_map(|id| self.current_manual_for(id).map(|manual| (id, manual)))
            .collect()
    }

    pub(crate) fn end_crop_drag(&mut self, cx: &mut Context<Self>) {
        if self.crop_drag.take().is_some() {
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 缝隙宽度
    // ------------------------------------------------------------------

    /// 在作品左侧的缝隙控件上按下：记下按下时的缝隙宽度、指针纵坐标和画布缩放。
    ///
    /// 只有纵向拖动，所以只记纵坐标；两种模式共用同一个控件，不关心按在哪条接缝上。
    pub(crate) fn begin_gap_drag(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let layout = self.canvas_layout();
        let gap = self.project.settings.gap;
        let start_gap_px = if gap.enabled { gap.px } else { 0 };
        self.gap_drag = Some(GapDragState {
            start_gap_px,
            start_pointer: f32::from(event.position.y),
            scale: layout.scale,
            undo_recorded: false,
            pointer: event.position,
        });
        cx.notify();
    }

    /// 拖动中：指针往上/往下挪多少，缝隙就按画布缩放换算着改多少。
    /// 气泡每帧跟随指针，所以即使间隙数值没变也要重画。
    pub(crate) fn drag_gap(&mut self, event: &DragMoveEvent<GapDrag>, cx: &mut Context<Self>) {
        let Some((start_pointer, scale, start_gap_px)) = self
            .gap_drag
            .as_ref()
            .map(|state| (state.start_pointer, state.scale, state.start_gap_px))
        else {
            return;
        };
        let pointer = event.event.position;
        let delta = f32::from(pointer.y) - start_pointer;
        let raw = start_gap_px as f32 + delta / scale.max(0.001);
        let next = tokens::clamp_gap_px(raw.max(0.0).round() as u32);
        if next != self.project.settings.gap.effective_px() {
            // 一次拖动只记一次撤销，中途连续的整数变化不各记一笔。
            if !self
                .gap_drag
                .as_ref()
                .is_some_and(|state| state.undo_recorded)
            {
                self.record_undo();
                if let Some(state) = self.gap_drag.as_mut() {
                    state.undo_recorded = true;
                }
            }
            self.project.settings.gap = self.project.settings.gap.with_px(next);
        }
        if let Some(state) = self.gap_drag.as_mut() {
            state.pointer = pointer;
        }
        cx.notify();
    }

    pub(crate) fn end_gap_drag(&mut self, cx: &mut Context<Self>) {
        // 气泡只在拖动期间存在，松手即随状态一起消失，不留淡出状态机。
        if self.gap_drag.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn cycle_gap_color(&mut self, cx: &mut Context<Self>) {
        let gap = self.project.settings.gap;
        let next = GapSetting::next_color(gap.color);
        self.record_undo();
        let start = match (self.prev_gap_color, self.gap_color_started_at) {
            (Some(from), Some(started_at)) => {
                let progress = (started_at.elapsed().as_secs_f32() / 0.15).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - progress).powi(5);
                crate::ui::theme::lerp_hsla(from, crate::ui::widgets::quad_color(gap.color), eased)
            }
            _ => crate::ui::widgets::quad_color(gap.color),
        };
        self.prev_gap_color = Some(start);
        self.gap_color_started_at = Some(std::time::Instant::now());
        self.gap_color_gen = self.gap_color_gen.wrapping_add(1);
        let generation = self.gap_color_gen;
        self.project.settings.gap.color = next;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.gap_color_gen == generation {
                    this.prev_gap_color = None;
                    this.gap_color_started_at = None;
                    cx.notify();
                }
            });
        })
        .detach();
        let message = self.language.gap_color_switched(next);
        self.set_status(message, false, cx);
    }

    // ------------------------------------------------------------------
    // 拉框多选
    // ------------------------------------------------------------------

    pub(crate) fn canvas_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(cx);
        if self.project.items.is_empty() {
            return;
        }
        let layout = self.canvas_layout();
        let position = self.content_position(event.position, window);
        // 图块、接缝和作品左侧的缝隙控件都不算“空白处”，按下去不要拖动窗口。
        if layout.block_at(position).is_some()
            || layout.seam_at(position).is_some()
            || layout
                .gap_control_rect()
                .is_some_and(|rect| rect.contains(position))
        {
            return;
        }
        // 空白处左键：先清空选择，然后交给系统拖动整个窗口。
        self.selected = None;
        self.selected_ids.clear();
        #[cfg(test)]
        {
            self.window_move_requests += 1;
        }
        cx.notify();
        window.start_window_move();
    }

    pub(crate) fn canvas_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging.is_some() && self.delete_hover {
            self.set_delete_hover(false, cx);
        }
        if event.pressed_button != Some(MouseButton::Right) {
            return;
        }
        let Some(pending) = self.right_pending else {
            return;
        };
        let position = self.content_position(event.position, window);
        if self.marquee.is_none() {
            let distance = ((position.x - pending.start_content.x).powi(2)
                + (position.y - pending.start_content.y).powi(2))
            .sqrt();
            if distance < tokens::MARQUEE_MIN_DRAG {
                return;
            }
            self.marquee = Some(MarqueeDrag {
                start: pending.start_content,
                current: position,
                moved: true,
                additive: pending.additive,
            });
        }
        self.update_marquee(position, cx);
    }

    /// 按当前指针位置更新选框，并重新计算命中的图块。
    fn update_marquee(&mut self, position: gpui::Point<f32>, cx: &mut Context<Self>) {
        let Some((start, additive)) = self
            .marquee
            .as_ref()
            .map(|marquee| (marquee.start, marquee.additive))
        else {
            return;
        };
        if let Some(marquee) = self.marquee.as_mut() {
            marquee.current = position;
            marquee.moved = true;
        }
        let layout = self.canvas_layout();
        let entries: Vec<(ItemId, Rect)> = layout
            .entries
            .iter()
            .map(|entry| (entry.id, entry.rect))
            .collect();
        let hits = marquee_hits(Rect::from_corners(start, position), &entries);
        if additive {
            for id in &hits {
                self.selected_ids.insert(*id);
            }
        } else {
            self.selected_ids = hits.iter().copied().collect();
        }
        self.selected = hits
            .last()
            .copied()
            .or_else(|| self.selected_ids.iter().next().copied());
        self.crop_drag = None;
        cx.notify();
    }

    pub(crate) fn canvas_mouse_up(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 左键空白处只负责拖动窗口，选择已经在按下时清空。
        self.clear_drag_state(cx);
    }

    pub(crate) fn canvas_right_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(cx);
        let position = self.content_position(event.position, window);
        let x = f32::from(event.position.x);
        let y = f32::from(event.position.y);
        let layout = self.canvas_layout();
        if let Some(id) = layout.block_at(position) {
            self.open_image_menu(id, x, y, cx);
            return;
        }
        // 缝隙控件上的右键由控件自己处理，别在这里弹出画布卡片。
        if layout
            .gap_control_rect()
            .is_some_and(|rect| rect.contains(position))
        {
            return;
        }
        // 空白处先记下按下位置：拖动超过阈值就框选，没拖动就抬起则弹画布卡片。
        self.right_pending = Some(RightPress {
            start_content: position,
            start_window: event.position,
            additive: event.modifiers.shift,
        });
    }

    pub(crate) fn canvas_right_up(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_marquee = self.marquee.take().is_some();
        let pending = self.right_pending.take();
        if had_marquee {
            cx.notify();
            return;
        }
        if let Some(pending) = pending {
            self.open_canvas_menu(
                f32::from(pending.start_window.x),
                f32::from(pending.start_window.y),
                cx,
            );
        }
    }

    pub(crate) fn canvas_right_up_out(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_marquee = self.marquee.take().is_some();
        let had_pending = self.right_pending.take().is_some();
        if had_marquee || had_pending {
            cx.notify();
        }
    }

    pub(crate) fn canvas_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.zoom_by(lines.signum() * tokens::ZOOM_STEP, cx);
        cx.stop_propagation();
    }

    // ------------------------------------------------------------------
    // 右键卡片
    // ------------------------------------------------------------------

    pub(crate) fn open_image_menu(&mut self, id: ItemId, x: f32, y: f32, cx: &mut Context<Self>) {
        if !self.selected_ids.contains(&id) {
            self.select(id, cx);
        }
        self.language_submenu_open = false;
        self.menu = Some(MenuState {
            kind: MenuKind::Image(id),
            x,
            y,
        });
        cx.notify();
    }

    pub(crate) fn open_canvas_menu(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.language_submenu_open = false;
        self.menu = Some(MenuState {
            kind: MenuKind::Canvas,
            x,
            y,
        });
        cx.notify();
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.language_submenu_open = false;
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 拖动排序与删除区
    // ------------------------------------------------------------------

    /// 拖动排序时按指针位置决定要不要挂上贴边自动滚动。
    ///
    /// 指针可能停在边缘完全不抖，所以不能只在 `on_drag_move` 里滚一次：
    /// 真的进入边缘带就挂一个计时器持续推，离开边缘或拖动结束就让它退出。
    pub(crate) fn refresh_auto_scroll(&mut self, cx: &mut Context<Self>) {
        // 拖着图去垃圾桶时不滚，否则底部的画布会一直动。
        let active = self.dragging.is_some()
            && !self.delete_hover
            && self.drag_pointer.is_some_and(|pointer| {
            let (x, y) = auto_scroll_intensity(
                point(f32::from(pointer.x), f32::from(pointer.y)),
                viewport_f32(self.viewport),
                tokens::AUTO_SCROLL_EDGE,
            );
            x != 0.0 || y != 0.0
        });
        self.set_auto_scroll(active, cx);
    }

    /// 按窗口坐标算这次拖动该插到哪张前面。
    ///
    /// 坐标换算、落点命中、拖动预览共用同一份 `CanvasLayout`：一次拖动移动原本要
    /// 算三遍排版，现在只算一遍。
    pub(crate) fn drag_target_for(
        &self,
        pointer: gpui::Point<gpui::Pixels>,
        viewport: gpui::Size<gpui::Pixels>,
        dragged: ItemId,
    ) -> Option<ItemId> {
        let layout = self.build_canvas_layout(DragPreview::Hidden(dragged), viewport);
        let origin = layout.origin(viewport, self.canvas_scroll.offset());
        let position = point(
            f32::from(pointer.x) - origin.x,
            f32::from(pointer.y) - origin.y,
        );
        layout.insertion_target(position)
    }

    /// 开关自动滚动任务；代号一变，还在等计时器的旧任务就会自己退出。
    fn set_auto_scroll(&mut self, active: bool, cx: &mut Context<Self>) {
        if active == self.auto_scroll_active {
            return;
        }
        self.auto_scroll_active = active;
        self.auto_scroll_gen = self.auto_scroll_gen.wrapping_add(1);
        if active {
            self.spawn_auto_scroll_tick(self.auto_scroll_gen, cx);
        }
    }

    fn spawn_auto_scroll_tick(&mut self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(
                    tokens::AUTO_SCROLL_INTERVAL_MS,
                ))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.auto_scroll_gen != generation || !this.auto_scroll_active {
                    return;
                }
                if this.step_auto_scroll(cx) {
                    this.spawn_auto_scroll_tick(generation, cx);
                } else {
                    this.auto_scroll_active = false;
                }
            });
        })
        .detach();
    }

    /// 推一帧自动滚动，并重算落点；返回是否还要继续。
    fn step_auto_scroll(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(id), Some(pointer)) = (self.dragging, self.drag_pointer) else {
            return false;
        };
        let viewport = self.viewport;
        let (x, y) = auto_scroll_intensity(
            point(f32::from(pointer.x), f32::from(pointer.y)),
            viewport_f32(viewport),
            tokens::AUTO_SCROLL_EDGE,
        );
        if x == 0.0 && y == 0.0 {
            return false;
        }
        let max = self.canvas_scroll.max_offset();
        let current = self.canvas_scroll.offset();
        let next = point(
            (current.x + gpui::px(x * tokens::AUTO_SCROLL_STEP))
                .clamp(-max.width, gpui::px(0.)),
            (current.y + gpui::px(y * tokens::AUTO_SCROLL_STEP))
                .clamp(-max.height, gpui::px(0.)),
        );
        if next == current {
            // 已经滚到头，等指针动一动再重新判断。
            return false;
        }
        self.canvas_scroll.set_offset(next);
        // 指针没动、内容动了，落点要按新的滚动位置重算，
        // 否则插入线会停在被推走的旧图块上。
        let target = self.drag_target_for(pointer, viewport, id);
        self.set_drag_target(id, target, cx);
        cx.notify();
        true
    }

    pub(crate) fn set_drag_target(
        &mut self,
        id: ItemId,
        target: Option<ItemId>,
        cx: &mut Context<Self>,
    ) {
        let changed = self.dragging != Some(id) || self.drag_over != Some(target);
        self.dragging = Some(id);
        self.drag_over = Some(target);
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn clear_drag_state(&mut self, cx: &mut Context<Self>) {
        if self.dragging.is_none() && self.drag_over.is_none() && !self.delete_hover {
            return;
        }
        self.dragging = None;
        self.drag_over = None;
        self.drag_pointer = None;
        self.delete_hover = false;
        self.auto_scroll_active = false;
        cx.notify();
    }

    pub(crate) fn set_delete_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.dragging.is_none() || self.delete_hover == hovered {
            return;
        }
        self.delete_hover = hovered;
        cx.notify();
    }

    pub(crate) fn drop_on_delete(&mut self, dragged: ItemId, cx: &mut Context<Self>) {
        let ids: Vec<ItemId> =
            if self.selected_ids.contains(&dragged) && self.selected_ids.len() > 1 {
                self.selected_ids.iter().copied().collect()
            } else {
                vec![dragged]
            };
        if ids.is_empty() {
            return;
        }
        self.record_undo();
        let removed = ids.len();
        self.project.items.retain(|item| !ids.contains(&item.id));
        self.selected_ids.retain(|id| !ids.contains(id));
        if self.selected.is_some_and(|id| ids.contains(&id)) {
            self.selected = self.selected_ids.iter().next().copied();
        }
        self.drop_previews_for(&ids);
        self.clear_drag_state(cx);
        let message = if removed > 1 {
            self.language.deleted_selected_count(removed)
        } else {
            self.language.deleted_selected().to_string()
        };
        self.set_status(message, false, cx);
    }

    pub(crate) fn delete_from_menu(&mut self, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.delete_selected_images(cx);
    }

    /// 拖动排序：拖动图片时如果多选，整组一起移动。
    pub(crate) fn apply_drop(
        &mut self,
        dragged: ItemId,
        before: Option<ItemId>,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<ItemId> =
            if self.selected_ids.contains(&dragged) && self.selected_ids.len() > 1 {
                self.project
                    .items
                    .iter()
                    .filter(|item| self.selected_ids.contains(&item.id))
                    .map(|item| item.id)
                    .collect()
            } else {
                vec![dragged]
            };
        if before.is_some_and(|target| ids.contains(&target)) {
            return;
        }
        let old_layout = self.canvas_layout();
        let prev_positions = old_layout
            .entries
            .iter()
            .map(|entry| (entry.id, (entry.rect.x, entry.rect.y)))
            .collect();
        let prev_seams = old_layout
            .seams
            .iter()
            .map(|seam| (seam.index, (seam.rect.x, seam.rect.y)))
            .collect();
        self.record_undo();
        // 顺序完全交给 reorder_ids（与松手前预览共用同一份逻辑），逐张按 id 取回。
        let order: Vec<ItemId> = self.project.items.iter().map(|item| item.id).collect();
        let new_order = crate::model::reorder_ids(&order, &ids, before);
        let mut by_id: std::collections::HashMap<ItemId, _> =
            self.project.items.drain(..).map(|item| (item.id, item)).collect();
        self.project.items = new_order
            .into_iter()
            .filter_map(|id| by_id.remove(&id))
            .collect();
        let generation = self
            .settle
            .as_ref()
            .map(|settle| settle.generation + 1)
            .unwrap_or(1);
        self.settle = Some(crate::app::DropSettle {
            generation,
            from: prev_positions,
            from_seams: prev_seams,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(160))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .settle
                    .as_ref()
                    .is_some_and(|settle| settle.generation == generation)
                {
                    this.settle = None;
                    cx.notify();
                }
            });
        })
        .detach();
        self.drag_over = None;
        self.dragging = None;
        self.drag_pointer = None;
        // 换顺序只改图块位置，图片和显示宽都没变，预览位图继续用。
        // 这里清缓存会让松手后的落位动画重新生成全部预览，白卡一下。
        self.set_status(self.language.reordered(), false, cx);
    }

    // ------------------------------------------------------------------
    // 出血位滑条拖动
    // ------------------------------------------------------------------

    pub(crate) fn drag_canvas_corner_handle(
        &mut self,
        event: &DragMoveEvent<CanvasCornerDrag>,
        cx: &mut Context<Self>,
    ) {
        let layout = self.canvas_layout();
        let handle = f32::from(tokens::size::RESIZE_HANDLE);
        let origin = event.bounds.origin;
        let center_x = f32::from(origin.x) + handle / 2.0;
        let center_y = f32::from(origin.y) + handle / 2.0;
        let width = layout.width + (f32::from(event.event.position.x) - center_x);
        let height = layout.height + (f32::from(event.event.position.y) - center_y);
        self.drag_canvas_corner(width, height, cx);
    }

    pub(crate) fn drag_bleed(&mut self, event: &DragMoveEvent<BleedDrag>, cx: &mut Context<Self>) {
        let width = f32::from(event.bounds.size.width).max(1.0);
        let x = f32::from(event.event.position.x) - f32::from(event.bounds.origin.x);
        let ratio = (x / width).clamp(0.0, 1.0);
        let index = (ratio * (tokens::BLEED_STEPS as f32 - 1.0)).round() as usize;
        self.set_bleed_index(index, cx);
    }
}
