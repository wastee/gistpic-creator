//! 画布手势与右键卡片的交互逻辑。
//!
//! 这一层只改应用状态；画布怎么画在 ui/canvas.rs，状态字段在 app.rs。

use gpui::{
    Context, DragMoveEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ScrollDelta,
    ScrollWheelEvent, Window, point,
};

use crate::app::{
    AppRoot, BleedDrag, CanvasCornerDrag, CropDragState, CropHandleDrag, GapDragState, MarqueeDrag,
    MenuKind, MenuState, SeamAxis, SeamDrag,
};
use crate::model::{GapSetting, ItemId, ManualBand};
use crate::ui::canvas::{CanvasLayout, Rect, marquee_hits};
use crate::ui::ghost_crop;
use crate::ui::tokens;

impl AppRoot {
    pub(crate) fn canvas_layout(&self) -> CanvasLayout {
        CanvasLayout::new(&self.project, self.zoom)
    }

    /// 把窗口坐标换成作品内容坐标。
    fn content_position(
        &self,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
    ) -> gpui::Point<f32> {
        let layout = self.canvas_layout();
        let origin = layout.origin(window.viewport_size(), self.canvas_scroll.offset());
        point(
            f32::from(position.x) - origin.x,
            f32::from(position.y) - origin.y,
        )
    }

    // ------------------------------------------------------------------
    // 悬停
    // ------------------------------------------------------------------

    pub(crate) fn set_hover_item(&mut self, id: ItemId, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(id)
        } else {
            self.hover_item.filter(|current| *current != id)
        };
        if self.hover_item != next {
            self.hover_item = next;
            cx.notify();
        }
    }

    pub(crate) fn set_hover_seam(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(index)
        } else {
            self.hover_seam.filter(|current| *current != index)
        };
        if self.hover_seam != next {
            self.hover_seam = next;
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 裁剪线
    // ------------------------------------------------------------------

    pub(crate) fn begin_crop_drag(
        &mut self,
        drag: CropHandleDrag,
        event: &MouseDownEvent,
        _window: &Window,
        cx: &mut Context<Self>,
    ) {
        let layout = self.canvas_layout();
        let Some(entry) = layout.entry(drag.id).copied() else {
            return;
        };
        let Some(item) = self.project.items.iter().find(|item| item.id == drag.id) else {
            return;
        };
        let (width, height, has_block) = (item.width, item.height, item.block.is_some());
        if !has_block {
            return;
        }
        let Some(start) = self.current_manual_for(drag.id) else {
            return;
        };
        if !self.selected_ids.contains(&drag.id) {
            self.select(drag.id, cx);
        }
        let scale = entry.display_width / width.max(1) as f32;
        self.crop_drag = Some(CropDragState {
            id: drag.id,
            edge: drag.edge,
            start,
            start_pointer_y: f32::from(event.position.y),
            source_origin_y: entry.rect.y - start.top * height as f32 * scale,
            auto_scroll_pixels: 0.0,
            image_height: height,
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
        let start_pointer_y = state.start_pointer_y;
        let image_height = state.image_height;
        let scale = state.scale;
        let auto_scroll_pixels = state.auto_scroll_pixels;
        let start = state.start;
        let viewport_height = window.viewport_size().height;
        let pointer_y = event.event.position.y;
        let delta = f32::from(pointer_y) - start_pointer_y;
        let scroll_delta = match drag.edge {
            crate::app::ManualEdge::Top if delta < 0.0 && pointer_y < gpui::px(96.) => {
                gpui::px(12.)
            }
            crate::app::ManualEdge::Bottom
                if delta > 0.0 && pointer_y > viewport_height - gpui::px(96.) =>
            {
                -gpui::px(12.)
            }
            _ => gpui::px(0.),
        };
        if scroll_delta != gpui::px(0.) {
            let scroll = self.canvas_scroll.offset();
            let max_scroll = self.canvas_scroll.max_offset();
            let next_y = (scroll.y + scroll_delta).clamp(-max_scroll.height, gpui::px(0.));
            self.canvas_scroll.set_offset(gpui::point(scroll.x, next_y));
            if let Some(state) = self.crop_drag.as_mut() {
                state.auto_scroll_pixels += f32::from(scroll_delta);
            }
        }
        let effective_scroll = auto_scroll_pixels + f32::from(scroll_delta);
        let delta_ratio = (delta - effective_scroll) / (image_height as f32 * scale).max(1.0);
        let raw = match drag.edge {
            crate::app::ManualEdge::Top => start.top + delta_ratio,
            crate::app::ManualEdge::Bottom => start.bottom + delta_ratio,
        };
        let bleed = self.project.settings.bleed;
        let Some(item) = self.project.items.iter().find(|item| item.id == drag.id) else {
            return;
        };
        let snapped = ghost_crop::snap_crop_ratio_with_display(
            item,
            drag.edge,
            raw,
            bleed,
            image_height as f32 * scale,
        );
        let band = match drag.edge {
            crate::app::ManualEdge::Top => {
                ManualBand::clamped(snapped.ratio.min(start.bottom - 0.005), start.bottom)
            }
            crate::app::ManualEdge::Bottom => {
                ManualBand::clamped(start.top, snapped.ratio.max(start.top + 0.005))
            }
        };
        let Some(band) = band else {
            return;
        };
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
        if let Some(state) = self.crop_drag.as_mut() {
            state.snap = snapped.tier;
        }
        let targets: Vec<ItemId> = if self.selected_ids.len() > 1 {
            self.selected_ids.iter().copied().collect()
        } else {
            vec![drag.id]
        };
        self.apply_band_to_items(&targets, band);
        cx.notify();
    }

    pub(crate) fn end_crop_drag(&mut self, cx: &mut Context<Self>) {
        if self.crop_drag.take().is_some() {
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 接缝
    // ------------------------------------------------------------------

    pub(crate) fn begin_gap_drag(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        _window: &Window,
        cx: &mut Context<Self>,
    ) {
        let layout = self.canvas_layout();
        let Some(seam) = layout.seams.iter().find(|seam| seam.index == index) else {
            return;
        };
        let gap = self.project.settings.gap;
        let start_gap_px = if gap.enabled { gap.px } else { 0 };
        self.gap_drag = Some(GapDragState {
            index,
            axis: seam.axis,
            start_gap_px,
            start_pointer: match seam.axis {
                SeamAxis::Vertical => f32::from(event.position.y),
                SeamAxis::Horizontal => f32::from(event.position.x),
            },
            scale: layout.scale,
            undo_recorded: false,
        });
        cx.notify();
    }

    pub(crate) fn drag_seam(
        &mut self,
        index: usize,
        event: &DragMoveEvent<SeamDrag>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.gap_drag.as_ref() else {
            return;
        };
        if state.index != index {
            return;
        }
        let pointer = match state.axis {
            SeamAxis::Vertical => f32::from(event.event.position.y),
            SeamAxis::Horizontal => f32::from(event.event.position.x),
        };
        let delta = pointer - state.start_pointer;
        let raw = state.start_gap_px as f32 + delta / state.scale.max(0.001);
        let next = tokens::clamp_gap_px(raw.max(0.0).round() as u32);
        if next == self.project.settings.gap.effective_px()
            && (next == 0) == !self.project.settings.gap.enabled
        {
        } else {
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
            cx.notify();
        }
    }

    pub(crate) fn end_gap_drag(&mut self, cx: &mut Context<Self>) {
        if self.gap_drag.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn cycle_gap_color(&mut self, cx: &mut Context<Self>) {
        let gap = self.project.settings.gap;
        if !gap.enabled || gap.px == 0 {
            return;
        }
        let next = GapSetting::next_color(gap.color);
        self.record_undo();
        self.project.settings.gap.color = next;
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
        if layout.block_at(position).is_some() || layout.seam_at(position).is_some() {
            return;
        }
        self.marquee = Some(MarqueeDrag {
            start: position,
            current: position,
            moved: false,
            additive: event.modifiers.shift,
        });
        cx.notify();
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
        if self.marquee.is_none() {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let position = self.content_position(event.position, window);
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        let distance = ((position.x - marquee.start.x).powi(2)
            + (position.y - marquee.start.y).powi(2))
        .sqrt();
        marquee.current = position;
        if distance >= tokens::MARQUEE_MIN_DRAG {
            marquee.moved = true;
        }
        let moved = marquee.moved;
        let additive = marquee.additive;
        let start = marquee.start;
        let current = marquee.current;
        if moved {
            let layout = self.canvas_layout();
            let entries: Vec<(ItemId, Rect)> = layout
                .entries
                .iter()
                .map(|entry| (entry.id, entry.rect))
                .collect();
            let hits = marquee_hits(Rect::from_corners(start, current), &entries);
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
        }
        cx.notify();
    }

    pub(crate) fn canvas_mouse_up(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(marquee) = self.marquee.take() {
            if !marquee.moved {
                self.selected = None;
                self.selected_ids.clear();
            }
            cx.notify();
        }
        self.clear_drag_state(cx);
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

    pub(crate) fn canvas_right_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(cx);
        let position = self.content_position(event.position, window);
        let x = f32::from(event.position.x);
        let y = f32::from(event.position.y);
        if let Some(id) = self.canvas_layout().block_at(position) {
            self.open_image_menu(id, x, y, cx);
        } else {
            self.open_canvas_menu(x, y, cx);
        }
    }

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

    pub(crate) fn next_item_after(&self, id: ItemId) -> Option<ItemId> {
        let index = self.project.items.iter().position(|item| item.id == id)?;
        self.project.items.get(index + 1).map(|item| item.id)
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
        self.delete_hover = false;
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
        self.preview_cache.clear();
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
        self.record_undo();
        let mut rest = Vec::with_capacity(self.project.items.len());
        let mut moving = Vec::with_capacity(ids.len());
        for item in self.project.items.drain(..) {
            if ids.contains(&item.id) {
                moving.push(item);
            } else {
                rest.push(item);
            }
        }
        let insert_at = match before {
            Some(target) => rest
                .iter()
                .position(|item| item.id == target)
                .unwrap_or(rest.len()),
            None => rest.len(),
        };
        for (offset, item) in moving.into_iter().enumerate() {
            rest.insert(insert_at + offset, item);
        }
        self.project.items = rest;
        self.drag_over = None;
        self.dragging = None;
        self.preview_cache.clear();
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
