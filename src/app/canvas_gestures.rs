//! 画布手势与右键卡片的交互逻辑。
//!
//! 这一层只改应用状态；画布怎么画在 ui/canvas.rs，状态字段在 app.rs。

use gpui::{
    Context, DragMoveEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ScrollDelta,
    ScrollWheelEvent, Window, point,
};

use crate::app::crop_drag;
use crate::app::{
    AppRoot, BleedDrag, CropDragState, CropHandleDrag, GapDrag, GapDragState, ManualEdge,
    MarqueeDrag, MenuKind, MenuState, NavigatorDrag, NavigatorDragState, RightPress,
};
use crate::model::{GapSetting, ItemId, ManualBand};
use crate::ui::canvas::{
    AxisPin, CanvasLayout, DragPreview, FrameAnchor, Rect, auto_scroll_intensity, marquee_hits,
};
use crate::ui::ghost_crop;
use crate::ui::tokens;

/// 需求 6：按住鼠标滚轮平移画布时记下的起点。
///
/// `start_pointer` 是按下那一刻指针在窗口里的位置，`start_offset` 是那一刻画布的
/// 滚动偏移。每一帧都按「现在的指针减按下时的指针」重新算，不做累加，
/// 免得把每帧的取整误差攒起来、拖久了画布跑偏。
#[derive(Clone, Copy)]
pub(crate) struct MiddlePan {
    pub start_pointer: gpui::Point<gpui::Pixels>,
    pub start_offset: gpui::Point<gpui::Pixels>,
}

/// 视口尺寸换成纯 `f32`，几何计算不关心 `Pixels` 包装。
/// 这一步算出来的保留带，和此刻画面上生效的保留带比，有没有真的改到东西。
///
/// 全都没变就返回 false，调用方据此不推进自动滚动、不记撤销。
/// 条数对不上说明有图算不出保留带，这种时候按「有变化」处理，别误挡。
fn step_changes_anything(live: &[(ItemId, ManualBand)], bands: &[(ItemId, ManualBand)]) -> bool {
    if live.len() != bands.len() {
        return true;
    }
    !bands
        .iter()
        .zip(live.iter())
        .all(|((band_id, band), (live_id, live_band))| band_id == live_id && band == live_band)
}

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
        let layout = CanvasLayout::new(
            &self.project,
            self.view_scale(),
            preview,
            viewport,
            self.pending_sizes.iter().copied(),
        );
        match self.crop_drag.as_ref() {
            Some(drag) => layout
                .freeze_frame(drag.frame)
                .freeze_scroll_extent(drag.frozen_max_scroll),
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
        // 拖动裁剪线会改作品宽高。自动适应会跟着重算比例，画面于是在拖动中缩放、
        // 跳位，看起来像「调边缘把缩放也调了」。按下这一刻把当前生效的比例定成
        // 手动比例，拖动全程和松手之后都用它；想回到自动适应要点「适合画布」。
        if self.scale_mode.is_fit() {
            self.set_manual_scale(layout.scale, cx);
        }
        if !self.selected_ids.contains(&drag.id) {
            self.select(drag.id, cx);
        }
        let targets = self.crop_drag_targets(drag.id, drag.edge);
        let band = entry.band;
        // 冻结按下时的显示比例：基准尺寸按作品算，不固定的话拖动改尺寸会让
        // 比例每帧重算，画面跟着缩放。
        let scale = point(
            entry.display_width / band.width().max(1) as f32,
            entry.display_height / band.height().max(1) as f32,
        );
        // 拖哪条边就钉住对面那条边：拖上边钉下边、拖下边钉上边，左右同理。
        // 另一条轴钉住起点，竖直拖动时画面不会横向漂、横向拖动时不会纵向漂。
        let viewport = window.viewport_size();
        let padding_start = layout.top_padding(viewport);
        let left_start = layout.left_padding(viewport);
        // 被拖边所在的那条轴，钉住被拖这张图的对面那条边（值 = 留白 + 那条边在
        // 画布里的位置）；另一条轴钉住画布起点，画面不会往反方向漂。
        let edge_pin = |edge: ManualEdge| {
            let (edge, position) = match edge {
                ManualEdge::Top => (ManualEdge::Bottom, entry.rect.bottom()),
                ManualEdge::Bottom => (ManualEdge::Top, entry.rect.y),
                ManualEdge::Left => (ManualEdge::Right, entry.rect.right()),
                ManualEdge::Right => (ManualEdge::Left, entry.rect.x),
            };
            (edge, position)
        };
        let frame = FrameAnchor {
            vertical: match drag.edge {
                ManualEdge::Top | ManualEdge::Bottom => {
                    let (edge, position) = edge_pin(drag.edge);
                    AxisPin::Edge {
                        id: drag.id,
                        edge,
                        start: padding_start + position,
                    }
                }
                _ => AxisPin::Canvas(padding_start),
            },
            horizontal: match drag.edge {
                ManualEdge::Left | ManualEdge::Right => {
                    let (edge, position) = edge_pin(drag.edge);
                    AxisPin::Edge {
                        id: drag.id,
                        edge,
                        start: left_start + position,
                    }
                }
                _ => AxisPin::Canvas(left_start),
            },
        };
        // 按下时的可滚动范围。拖动中作品会被裁小，不把它钉住的话，滚动区每帧
        // 都把偏移夹回新范围，画面会一股一股地往作品开头跑。
        let frozen_max_scroll = layout.max_scroll(viewport);
        self.crop_drag = Some(CropDragState {
            id: drag.id,
            edge: drag.edge,
            start,
            targets,
            start_pointer: point(f32::from(event.position.x), f32::from(event.position.y)),
            frame,
            scroll_start: point(
                f32::from(self.canvas_scroll.offset().x),
                f32::from(self.canvas_scroll.offset().y),
            ),
            frozen_max_scroll,
            auto_scroll_pixels: point(0.0, 0.0),
            image_size,
            scale,
            undo_recorded: false,
            snap: None,
        });
        // 需求 10：开始拖动就把边缘提示顶到满格。
        self.update_edge_hint(cx);
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
        let scroll_start = state.scroll_start;
        let targets = state.targets.clone();
        let viewport = window.viewport_size();
        let pointer = event.event.position;
        let delta = point(
            f32::from(pointer.x) - start_pointer.x,
            f32::from(pointer.y) - start_pointer.y,
        );

        // 需求 2：这条边朝图片外面已经顶住（图片在这个方向已经完整显示），
        // 而指针还在继续往外拖，那么这一拖什么都不会发生：裁剪带不变，画面也
        // 不该动，自动滚动更不该推进。没有这一步时，图片明明已经完整显示，
        // 指针拖到视口边缘，自动滚动照样把画面推向作品开头，看起来就是
        // 「调第三张的边，画面突然跳到第一张」。
        //
        // 只按朝外的方向判断：顶住但指针往内拖时不能提前退出，否则往内也拖不动。
        // 朝向：上和左是负方向，下和右是正方向；上下边看纵坐标，左右边看横坐标。
        let outward = if edge.is_columns() {
            match edge {
                ManualEdge::Left => delta.x < 0.0,
                _ => delta.x > 0.0,
            }
        } else {
            match edge {
                ManualEdge::Top => delta.y < 0.0,
                _ => delta.y > 0.0,
            }
        };
        if outward && !targets.is_empty() {
            // 拿「画面上此刻生效的保留带」判断顶没顶住，不能拿按下时的值：
            // 按下时还有裁切的边，往外拖到图片边界的那一帧本身正是把边界补满的
            // 一帧，用按下时的旧值判断会把它当成已经顶住而丢掉，边就永远差最后
            // 一小步到不了位。
            let current: Vec<(ItemId, ManualBand)> = targets
                .iter()
                .filter_map(|(id, _)| self.current_manual_for(*id).map(|band| (*id, band)))
                .collect();
            // 条数对不上说明有图算不出保留带，这种时候按没顶住处理，别误挡。
            if current.len() == targets.len()
                && crop_drag::edge_is_saturated(&current, edge, self.project.mode)
            {
                return;
            }
        }

        // 靠近视口边缘时自动滚动，滚轮位移要从拖动量里扣掉，
        // 这样指针不动、画面在动的时候裁剪线也不会跟着漂。
        //
        // 触发距离和每帧步长都取 tokens 里的值，不在手势代码里另写一套数字：
        // 和拖动排序那边的自动滚动保持一致，改的时候只需要改一处。
        let edge_zone = gpui::px(tokens::AUTO_SCROLL_EDGE);
        let step = gpui::px(tokens::AUTO_SCROLL_STEP);
        let mut scroll_step = point(gpui::px(0.), gpui::px(0.));
        if edge.is_columns() {
            if delta.x < 0.0 && pointer.x < edge_zone {
                scroll_step.x = step;
            } else if delta.x > 0.0 && pointer.x > viewport.width - edge_zone {
                scroll_step.x = -step;
            }
        } else if delta.y < 0.0 && pointer.y < edge_zone {
            scroll_step.y = step;
        } else if delta.y > 0.0 && pointer.y > viewport.height - edge_zone {
            scroll_step.y = -step;
        }
        // 这一步要滚多少先算出来，不在这里就改画面：真正的提交放到下面
        // 「这一步有没有产生变化」判完之后。候选值照旧参与 delta_ratio 的计算，
        // 所以算法没变，只是把「改滚动位置」这个动作推迟到确定要改数据之后。
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

        let bands = crop_drag::drag_bands(&targets, edge, effective_delta, self.project.mode);
        if bands.is_empty() {
            return;
        }
        // 这一步到底有没有产生变化：拿算出来的保留带和「此刻画面上生效的保留带」
        // 比，一样就说明这一拖什么都没改，直接返回，不推进自动滚动、不记撤销。
        //
        // 这里必须和上面那条饱和守卫用同一个来源（都是 current_manual_for），
        // 否则就是两套判据打架：饱和守卫按整组判、吸附按被拖那张判。被拖那张
        // 已经贴住边时（比如它的上边本来就是 0.0），吸附会把 effective_delta 钉成
        // 0，整组保留带原地不动；整组守卫又因为组里别的图还有余量而不触发，
        // 代码就一路走到自动滚动，画面持续滚向作品开头，看起来正是
        // 「调第三张的边，画面突然跳到第一张」。
        //
        // 拿「此刻生效的保留带」比而不是拿 item.manual 比：自动算出来的图
        // item.manual 是 None，可它此刻生效的保留带已经是整张，拿 None 去比
        // 会误判成「有变化」，第一帧照样会滚。
        let live: Vec<(ItemId, ManualBand)> = targets
            .iter()
            .filter_map(|(id, _)| self.current_manual_for(*id).map(|band| (*id, band)))
            .collect();
        if !step_changes_anything(&live, &bands) {
            return;
        }

        // 到这里才确定这一步真的会改数据，此时才记下自动滚动量。
        // 真正写滚动偏移放到改完保留带之后：画面还要按锚点重新贴住对边，
        // 两笔合成一次写，滚动范围也按新的排版算，不会多滚一截。
        // 上限不回读 `ScrollHandle::max_offset()`：那是上一帧量出来的旧范围。
        if scroll_step != point(gpui::px(0.), gpui::px(0.)) {
            if let Some(state) = self.crop_drag.as_mut() {
                state.auto_scroll_pixels.x += f32::from(scroll_step.x);
                state.auto_scroll_pixels.y += f32::from(scroll_step.y);
            }
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
        // 拖边改了画布大小：按锚点把画面重贴回对边，拖动中只让被拖的那一侧移动。
        self.set_crop_anchor_scroll(scroll_start, viewport);
        // 预览位图只跟图片和显示宽有关，裁剪范围是靠界面裁切加位图位移做出来的，
        // 所以这里改裁剪范围不用清缓存——清了反而每拖一步都重画位图，拖尺寸就卡。
        cx.notify();
    }

    /// 这一拖要跟着改哪些图。
    ///
    /// 横排共用高度、竖排共用宽度，改共用那条边时整组一起改；其余情况只改被拖的那张
    /// （多选时改所有选中的图）。字幕模式一条一条改，多选时同步。
    ///
    /// 整组一起改是用户明确要的行为，不是缺陷、也不是过渡做法：拖 A 的边会同时改到
    /// B 的裁剪数据，这是设计本意。被拖的那张一定会包含在结果里，即使它不在选中集合里。
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

    /// 裁剪拖动中按锚点重贴画布：拖哪条边，就让对面那条边留在原来的屏幕位置。
    ///
    /// 位移先由留白吸收（钉下边时上留白跟着作品高度变），留白收到 0 之后
    /// 再由滚动补；滚动也到头时画面才会从反方向动。
    fn set_crop_anchor_scroll(
        &mut self,
        scroll_start: gpui::Point<f32>,
        viewport: gpui::Size<gpui::Pixels>,
    ) {
        let auto = self
            .crop_drag
            .as_ref()
            .map(|state| state.auto_scroll_pixels)
            .unwrap_or_else(|| point(0.0, 0.0));
        let layout = self.canvas_layout();
        // 留白收到 0 之后，钉住的那条边还差多少就靠滚动补多少；没到 0 时是 0。
        let (extra_x, extra_y) = layout.frozen_scroll_extra();
        let (max_x, max_y) = layout.max_scroll(viewport);
        let offset = point(
            (scroll_start.x + auto.x + extra_x).clamp(-max_x, 0.0),
            (scroll_start.y + auto.y + extra_y).clamp(-max_y, 0.0),
        );
        self.canvas_scroll
            .set_offset(point(gpui::px(offset.x), gpui::px(offset.y)));
    }

    pub(crate) fn end_crop_drag(&mut self, cx: &mut Context<Self>) {
        if self.crop_drag.take().is_some() {
            // 需求 10：松手之后边缘提示淡回默认的半透明状态。
            self.update_edge_hint(cx);
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
        // 需求 12/13：开始拖间隔时胶囊不透明，数值提示出现。
        self.update_gap_control_fade(cx);
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
        if self.gap_drag.take().is_some() {
            // 需求 13：松手后数值提示淡出。
            self.update_gap_control_fade(cx);
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
        // 没有图片时画布上只有空状态卡片，卡片自己会挡掉左键按下；走到这里说明
        // 按在卡片外的空白处，和导入图片后的空白处一样，应该能拖动整个窗口。
        // 没有图片时画布上只有空状态卡片，卡片自己会挡掉左键按下；走到这里说明
        // 按在卡片外的空白处，和导入图片后的空白处一样，应该能拖动整个窗口。
        if self.project.items.is_empty() {
            #[cfg(test)]
            {
                self.window_move_requests += 1;
            }
            window.start_window_move();
            return;
        }
        let layout = self.canvas_layout();
        let position = self.content_position(event.position, window);
        // 图块、接缝和作品左侧的缝隙控件都不算“空白处”，按下去不要拖动窗口。
        //
        // 需求 12：缝隙控件改在窗口坐标里定位，所以要单独拿窗口坐标比一次；
        // 用作品坐标比会永远落空，按在控件上会被当成空白处去拖窗口。
        let control_hit = layout
            .gap_control_rect(window.viewport_size(), self.canvas_scroll.offset())
            .is_some_and(|rect| {
                rect.contains(gpui::point(
                    f32::from(event.position.x),
                    f32::from(event.position.y),
                ))
            });
        if layout.block_at(position).is_some() || layout.seam_at(position).is_some() || control_hit
        {
            return;
        }
        // 空白处左键：先清空选择，然后交给系统拖动整个窗口。
        let from = self.selected_ids.clone();
        self.selected = None;
        self.selected_ids.clear();
        self.animate_selection_from(from, cx);
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
        // 需求 6：按住滚轮平移画布。偏移按「现在的指针减按下时的指针」算，
        // 所以画布内容跟着指针走：指针往右，内容往右。
        if event.pressed_button == Some(MouseButton::Middle)
            && let Some(pan) = self.middle_pan
        {
            let delta = event.position - pan.start_pointer;
            let next = pan.start_offset + delta;
            // `ScrollHandle::set_offset` 只负责存值，不夹范围（真正的夹紧在每一帧
            // 排版时做）。这里先按当前可滚动范围夹住，免得推出范围的值攒下来、
            // 松手后画面还得再弹回来。
            let max = self.canvas_scroll.max_offset();
            self.canvas_scroll.set_offset(point(
                next.x.clamp(-max.width, gpui::px(0.)),
                next.y.clamp(-max.height, gpui::px(0.)),
            ));
            // `set_offset` 只存值，不通知重绘。不在这里主动标脏，按住滚轮拖动时
            // 画面就不会连续跟着动，只有等别的绘制原因才更新。
            cx.notify();
            cx.stop_propagation();
            return;
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
        //
        // 拖动排序的状态不能在这里清：这条监听注册得比作品块的 on_drop 早，冒泡时
        // 先跑到它。在这里清掉 drag_over，作品块和窗口兜底的 drop 都拿不到落点，
        // 这次拖动就被静默丢掉。落点由 drop 自己接手，没被接手的由窗口那层兜底。
        if self.dragging.is_some() {
            return;
        }
        self.clear_drag_state(cx);
    }

    /// 需求 6：按住滚轮，开始平移画布。
    ///
    /// 只记下起点，真正的位移在 `canvas_mouse_move` 里算。这里绝不能调
    /// `window.start_window_move()`：那是左键按在空白处拖整个窗口用的，
    /// 滚轮平移只移动画布里的作品。
    pub(crate) fn canvas_middle_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.middle_pan = Some(MiddlePan {
            start_pointer: event.position,
            start_offset: self.canvas_scroll.offset(),
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// 需求 6：松开滚轮，结束画布平移。
    pub(crate) fn canvas_middle_up(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.middle_pan = None;
        cx.stop_propagation();
        cx.notify();
    }

    /// 需求 6：此刻是不是正在按住滚轮平移画布。界面据此把手形光标换成握紧的手。
    pub(crate) fn is_middle_panning(&self) -> bool {
        self.middle_pan.is_some()
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
        // 控件在窗口坐标里定位，这里也按窗口坐标比。
        let control_hit = layout
            .gap_control_rect(window.viewport_size(), self.canvas_scroll.offset())
            .is_some_and(|rect| {
                rect.contains(gpui::point(
                    f32::from(event.position.x),
                    f32::from(event.position.y),
                ))
            });
        if control_hit {
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
        self.release_previews_for(&ids);
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
                // 和位移动画用同一个时长：这个计时器只负责在动画结束后把
                // 临时状态清掉，两边各写一个数字的话，改一处就会脱节。
                .timer(crate::app::DropSettle::DURATION)
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

    pub(crate) fn drag_bleed(&mut self, event: &DragMoveEvent<BleedDrag>, cx: &mut Context<Self>) {
        let width = f32::from(event.bounds.size.width).max(1.0);
        let x = f32::from(event.event.position.x) - f32::from(event.bounds.origin.x);
        let ratio = (x / width).clamp(0.0, 1.0);
        let index = (ratio * (tokens::BLEED_STEPS as f32 - 1.0)).round() as usize;
        self.set_bleed_index(index, cx);
    }
}

impl AppRoot {
    /// 需求 15：算出导航器此刻要画的东西，供渲染和拖动共用。
    ///
    /// 返回 `(导航器矩形, 缩略图矩形, 视口框矩形)`，坐标都是窗口坐标。
    /// 作品没溢出、或者窗口放不下时返回 `None`，整块不画。
    pub(crate) fn navigator_geometry(
        &self,
        viewport: gpui::Size<gpui::Pixels>,
    ) -> Option<(Rect, Rect, Rect)> {
        let layout = self.canvas_layout();
        let max_scroll = layout.max_scroll(viewport);
        let nav = crate::ui::canvas::navigator_rect(
            viewport,
            gpui::Size {
                width: layout.width,
                height: layout.height,
            },
            max_scroll,
        )?;
        // 缩略图在窗口里的位置统一由导航器模块算，渲染和这里用同一个结果。
        let thumb = crate::ui::canvas::thumbnail_bounds(
            nav,
            gpui::Size {
                width: layout.width,
                height: layout.height,
            },
        );
        let scroll = self.canvas_scroll.offset();
        let frame = crate::ui::canvas::viewport_frame(
            thumb,
            gpui::Size {
                width: layout.width,
                height: layout.height,
            },
            gpui::Size {
                width: f32::from(viewport.width),
                height: f32::from(viewport.height),
            },
            gpui::point(f32::from(scroll.x), f32::from(scroll.y)),
        );
        Some((nav, thumb, frame))
    }

    /// 需求 15：在导航器上按下，开始拖视口框。
    pub(crate) fn begin_navigator_drag(
        &mut self,
        event: &MouseDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = window.viewport_size();
        let Some((_, _, frame)) = self.navigator_geometry(viewport) else {
            return;
        };
        // 几何一律用纯 `f32`：`Pixels` 和 `f32` 混着算编译不过，也容易看错单位。
        let pointer = gpui::point(f32::from(event.position.x), f32::from(event.position.y));
        // 按在框里就抓在按下那一点，按在缩略图别处就以框中心对齐过去，
        // 这样点哪儿都能把框叫过来，不用先精确点在框上。
        let inside = pointer.x >= frame.x
            && pointer.x <= frame.right()
            && pointer.y >= frame.y
            && pointer.y <= frame.bottom();
        let grab = if inside {
            gpui::point(pointer.x - frame.x, pointer.y - frame.y)
        } else {
            gpui::point(frame.w / 2.0, frame.h / 2.0)
        };
        self.navigator_drag = Some(NavigatorDragState { grab });
        if !inside {
            // 点在外面：立刻把框挪到指针处，手感上就是「跳过去」。
            self.apply_navigator_drag(pointer, viewport, cx);
        }
        self.update_navigator_fade(cx);
        cx.notify();
    }

    /// 需求 15：拖动中，按指针位置改画布的滚动偏移。
    pub(crate) fn drag_navigator(
        &mut self,
        event: &DragMoveEvent<NavigatorDrag>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = window.viewport_size();
        self.apply_navigator_drag(
            gpui::point(
                f32::from(event.event.position.x),
                f32::from(event.event.position.y),
            ),
            viewport,
            cx,
        );
    }

    /// 需求 15：把指针位置换算成滚动偏移并写进滚动区。
    ///
    /// 只改滚动位置：缩放和作品数据都不动。偏移由 `scroll_for_frame` 夹在有效
    /// 范围内，拖出界也不会滚过头。
    fn apply_navigator_drag(
        &mut self,
        pointer: gpui::Point<f32>,
        viewport: gpui::Size<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(grab) = self.navigator_drag.map(|state| state.grab) else {
            return;
        };
        let layout = self.canvas_layout();
        let max_scroll = layout.max_scroll(viewport);
        let Some((_, thumb, _)) = self.navigator_geometry(viewport) else {
            return;
        };
        let scrolled = crate::ui::canvas::scroll_for_frame(
            pointer,
            grab,
            thumb,
            gpui::Size {
                width: layout.width,
                height: layout.height,
            },
            gpui::Size {
                width: f32::from(viewport.width),
                height: f32::from(viewport.height),
            },
            max_scroll,
        );
        // 滚动偏移是负的：往下往右看就是往左往上滚。
        self.canvas_scroll
            .set_offset(gpui::point(gpui::px(-scrolled.x), gpui::px(-scrolled.y)));
        cx.notify();
    }

    /// 需求 15：松开视口框，结束拖动。
    pub(crate) fn end_navigator_drag(&mut self, cx: &mut Context<Self>) {
        if self.navigator_drag.take().is_some() {
            self.update_navigator_fade(cx);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{crop_drag, step_changes_anything};
    use crate::model::{ItemId, ManualBand, Mode};

    fn band(top: f32) -> ManualBand {
        ManualBand::clamped(0.0, 1.0, top, 1.0).expect("测试用的保留带应有效")
    }

    /// 本次要修的场景：横排三张图，第 1 张上边裁到 0.3，第 2、3 张完整。
    /// 被拖的是第 3 张，它的上边本来就是 0.0，吸附把这一步的拖动量钉成 0，
    /// 于是整组保留带一条都没变。这时候不能推进自动滚动，否则画面会一路
    /// 滚向第 1 张，而第 1 张的裁剪一动不动。
    #[test]
    fn a_pinned_step_does_not_count_as_a_change() {
        let live = [(1, band(0.3)), (2, band(0.0)), (3, band(0.0))];
        // effective_delta 为 0：整组原地不动，正是吸附钉住时的结果。
        let bands = crop_drag::drag_bands(&live, super::ManualEdge::Top, 0.0, Mode::Layout);
        assert!(
            !step_changes_anything(&live, &bands),
            "被拖那张被吸附钉住时这一步没有任何变化，不该推进自动滚动"
        );
    }

    /// 同一场景下，整组饱和守卫不会触发：第 1 张上边还有余量。
    /// 这条断言说明「整组顶住」和「这一步有没有变化」是两套判据，
    /// 光靠原来那条守卫盖不住这个场景，必须再叠一条。
    #[test]
    fn the_group_guard_alone_misses_this_scene() {
        let live = [(1, band(0.3)), (2, band(0.0)), (3, band(0.0))];
        assert!(
            !crop_drag::edge_is_saturated(&live, super::ManualEdge::Top, Mode::Layout),
            "第 1 张上边还有余量，整组饱和守卫这个场景不触发"
        );
    }

    /// 反过来：整组真的都顶住时，这一步同样没有变化，两条判据一致。
    #[test]
    fn a_fully_saturated_group_is_also_not_a_change() {
        let live = [(1, band(0.0)), (2, band(0.0)), (3, band(0.0))];
        assert!(crop_drag::edge_is_saturated(
            &live,
            super::ManualEdge::Top,
            Mode::Layout
        ));
        let bands = crop_drag::drag_bands(&live, super::ManualEdge::Top, -0.2, Mode::Layout);
        assert!(
            !step_changes_anything(&live, &bands),
            "整组都顶住时同样没有变化"
        );
    }

    /// 被拖那张有余量、整组也确实改得动时，这一步算有变化，自动滚动该推进。
    /// 要是这条反了，就变成「该滚的时候不滚」，把原来能用的功能弄坏。
    #[test]
    fn a_step_that_really_moves_a_band_counts_as_a_change() {
        let live = [(1, band(0.3)), (2, band(0.0)), (3, band(0.0))];
        let bands = crop_drag::drag_bands(&live, super::ManualEdge::Top, -0.05, Mode::Layout);
        assert!(
            step_changes_anything(&live, &bands),
            "第 1 张上边跟着往下放，这一步确实改了数据，该推进自动滚动"
        );
    }

    /// 条数对不上说明有图算不出保留带，按「有变化」处理，别误挡。
    #[test]
    fn a_length_mismatch_counts_as_a_change() {
        let live = [(1 as ItemId, band(0.3))];
        assert!(step_changes_anything(&live, &[]));
    }

    /// 图对不上号也算有变化，别把两张不同的图误判成没变。
    #[test]
    fn a_different_image_counts_as_a_change() {
        let live = [(1 as ItemId, band(0.3))];
        let bands = [(2 as ItemId, band(0.3))];
        assert!(step_changes_anything(&live, &bands));
    }
}
