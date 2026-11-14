//! 裁剪线拖动的规则：拖一条边改哪个轴、这一拖要不要带着同一排或同一列的图一起动、
//! 每张图各自的保留带怎么变。
//!
//! 这里只有纯计算，不碰界面、不碰状态，方便单独测试。另外还负责判断一条边朝外
//! 是不是已经顶住，供调用方判断「这一拖是不是什么都不会发生」。

use crate::model::{ItemId, LayoutTarget, ManualBand, Mode};

use super::ManualEdge;

/// 裁剪线改动的方向。上下两条边改纵向，左右两条边改横向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropAxis {
    Rows,
    Columns,
}

impl ManualEdge {
    pub fn axis(self) -> CropAxis {
        match self {
            ManualEdge::Top | ManualEdge::Bottom => CropAxis::Rows,
            ManualEdge::Left | ManualEdge::Right => CropAxis::Columns,
        }
    }
}

/// 这个模式下能拖哪几条裁剪线。
///
/// 字幕模式只裁纵向，所以只有上下两条；排版模式四个方向都能裁。
pub fn edges_for(mode: Mode) -> &'static [ManualEdge] {
    match mode {
        Mode::Subtitle => &[ManualEdge::Top, ManualEdge::Bottom],
        Mode::Layout => &[
            ManualEdge::Top,
            ManualEdge::Bottom,
            ManualEdge::Left,
            ManualEdge::Right,
        ],
    }
}

/// 横排时上下两条裁剪线、竖排时左右两条裁剪线，拖动时整组一起改。
///
/// 这是用户明确要求的行为：同一排（或同一列）的图算一组，拖组里任意一张的边，
/// 组内其它图按同样的量一起改。拖 A 的边会同时改到 B 的裁剪数据，正是这条规则
/// 的结果，属于设计本意。
///
/// 字幕模式改的是一条一条的裁剪线，多选时由调用方按选中集合处理。
pub fn group_follows(mode: Mode, target: LayoutTarget, axis: CropAxis) -> bool {
    if mode != Mode::Layout {
        return false;
    }
    matches!(
        (target, axis),
        (LayoutTarget::Row, CropAxis::Rows) | (LayoutTarget::Column, CropAxis::Columns)
    )
}

/// 判断「这条边朝图片外面还有没有余量」时用的试探拖动量。
///
/// 归一化比例的满量程就是 1.0，取一个不小于 1.0 的值：只要这条边朝外还有
/// 哪怕一点点余量，试探出来的保留带必然和现在的不同。
const OUTWARD_PROBE: f32 = 1.0;

/// 这条边朝图片外面是不是已经顶住了（图片在这个方向已经完整显示）。
///
/// 做法是拿一个朝外的大增量跑一遍 drag_bands，结果和现在的保留带一模一样，
/// 就说明继续往外拖不会产生任何变化。朝外的方向：上和左是负方向，下和右是正方向。
///
/// 调用方还要自己判断指针是不是在往外拖：顶住但指针往内拖时不能提前退出，
/// 否则往内拖也会被挡住。
pub fn edge_is_saturated(targets: &[(ItemId, ManualBand)], edge: ManualEdge) -> bool {
    let outward = match edge {
        ManualEdge::Top | ManualEdge::Left => -1.0,
        ManualEdge::Bottom | ManualEdge::Right => 1.0,
    };
    let probe = drag_bands(targets, edge, outward * OUTWARD_PROBE);
    // 先比条数再逐项比：条数对不上说明有图算不出保留带，那就不能当成顶住。
    probe.len() == targets.len()
        && probe
            .iter()
            .zip(targets.iter())
            .all(|((id, band), (start_id, start))| id == start_id && band == start)
}

/// 一次拖动里每张图各自的新保留带。
pub fn drag_bands(
    targets: &[(ItemId, ManualBand)],
    edge: ManualEdge,
    delta: f32,
) -> Vec<(ItemId, ManualBand)> {
    targets
        .iter()
        .filter_map(|(id, start)| shift_edge(*start, edge, delta).map(|band| (*id, band)))
        .collect()
}

/// 拖这一个方向的边时，这个方向至少要留下多少跨度。
///
/// 两个下限取更严的那个：本方向留 15%，同时整个保留范围的面积不低于 2.25%。
/// 另一个方向已经很窄时，光靠 15% 撑不住面积，这个方向就得留得更宽。
fn min_span(other: f32) -> f32 {
    ManualBand::MIN_KEEP_RATIO.max(ManualBand::MIN_KEEP_AREA / other.max(1e-6))
}

/// 这张图的这条边被拖动之后的新保留带。
///
/// `delta` 是归一化后的拖动量。四条边各自独立：拖上下边不动左右，拖左右边不动上下。
/// 已经贴着图片边界、或已经收到最小保留范围时，继续往同一个方向拖结果不变，
/// 调用方据此判定「这次拖动没产生任何变化」，不记撤销也不改数据。
pub fn shift_edge(start: ManualBand, edge: ManualEdge, delta: f32) -> Option<ManualBand> {
    if !delta.is_finite() {
        return None;
    }
    let (left, right, top, bottom) = (start.left, start.right, start.top, start.bottom);
    let min_height = min_span(right - left);
    let min_width = min_span(bottom - top);
    // 自动识别出来的裁剪带可能比最小保留范围还窄（一行字幕常常只占画面一成），
    // 这种情况下只许往外放，不许再往里收，否则用户一拖就会「跳」到 15%。
    match edge {
        ManualEdge::Top => {
            let limit = (bottom - min_height).max(0.0);
            let ceiling = if top >= limit { top } else { limit };
            ManualBand::clamped(left, right, (top + delta).clamp(0.0, ceiling), bottom)
        }
        ManualEdge::Bottom => {
            let limit = (top + min_height).min(1.0);
            let floor = if bottom <= limit { bottom } else { limit };
            ManualBand::clamped(left, right, top, (bottom + delta).clamp(floor, 1.0))
        }
        ManualEdge::Left => {
            let limit = (right - min_width).max(0.0);
            let ceiling = if left >= limit { left } else { limit };
            ManualBand::clamped((left + delta).clamp(0.0, ceiling), right, top, bottom)
        }
        ManualEdge::Right => {
            let limit = (left + min_width).min(1.0);
            let floor = if right <= limit { right } else { limit };
            ManualBand::clamped(left, (right + delta).clamp(floor, 1.0), top, bottom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(left: f32, right: f32, top: f32, bottom: f32) -> ManualBand {
        ManualBand::clamped(left, right, top, bottom).expect("测试用的保留带应有效")
    }

    #[test]
    fn bottom_edge_keeps_top_and_crops_bottom() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Bottom, -0.2).unwrap();
        assert_eq!(next.top, 0.0, "拖下边缘不能动上边缘");
        assert!((next.bottom - 0.8).abs() < 1e-6);
    }

    #[test]
    fn top_edge_keeps_bottom_and_crops_top() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 0.2).unwrap();
        assert!((next.top - 0.2).abs() < 1e-6);
        assert_eq!(next.bottom, 1.0, "拖上边缘不能动下边缘");
    }

    #[test]
    fn right_edge_keeps_left_and_crops_right() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Right, -0.25).unwrap();
        assert!((next.right - 0.75).abs() < 1e-6);
        assert_eq!(next.left, 0.0);
        // 横向拖动不能动纵向范围
        assert_eq!(next.top, 0.0);
        assert_eq!(next.bottom, 1.0);
    }

    #[test]
    fn left_edge_keeps_right_and_crops_left() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Left, 0.25).unwrap();
        assert!((next.left - 0.25).abs() < 1e-6);
        assert_eq!(next.right, 1.0);
    }

    #[test]
    fn dragging_outwards_when_whole_image_does_nothing() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        // 已经整张画面，继续往外拖不应该产生任何变化
        assert_eq!(shift_edge(start, ManualEdge::Bottom, 0.5), Some(start));
        assert_eq!(shift_edge(start, ManualEdge::Top, -0.5), Some(start));
        assert_eq!(shift_edge(start, ManualEdge::Right, 0.5), Some(start));
        assert_eq!(shift_edge(start, ManualEdge::Left, -0.5), Some(start));
    }

    /// 一直往一条边的方向拖，两条边也不会交叉。
    ///
    /// 本方向最少留 15%；另一方向已经很窄时，按面积下限算出来的跨度更大，
    /// 断言用的是这个更大的下限，不是写死的 15%。
    #[test]
    fn edges_cannot_cross_each_other() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 5.0).unwrap();
        assert!(
            next.bottom - next.top >= ManualBand::MIN_KEEP_RATIO - 1e-5,
            "上下边不能交叉，实际留下 {}",
            next.bottom - next.top
        );
        assert!(next.bottom > next.top, "上下边不能交叉");
        let next = shift_edge(start, ManualEdge::Left, 5.0).unwrap();
        assert!(
            next.right - next.left >= ManualBand::MIN_KEEP_RATIO - 1e-5,
            "左右边不能交叉，实际留下 {}",
            next.right - next.left
        );
        assert!(next.right > next.left, "左右边不能交叉");

        // 左右只剩 10%：纵向至少留 2.25% / 10% = 22.5%，否则两条边会挤到一起。
        let thin_columns = band(0.45, 0.55, 0.0, 1.0);
        let next = shift_edge(thin_columns, ManualEdge::Top, 5.0).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "另一方向只剩 10% 时，上下边应留到 22.5%，实际 {}",
            next.bottom - next.top
        );
        // 反过来：上下只剩 10%，左右边同样要留到 22.5%。
        let thin_rows = band(0.0, 1.0, 0.45, 0.55);
        let next = shift_edge(thin_rows, ManualEdge::Left, 5.0).unwrap();
        assert!(
            (next.right - next.left - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "另一方向只剩 10% 时，左右边应留到 22.5%，实际 {}",
            next.right - next.left
        );
    }

    #[test]
    fn subtitle_mode_has_no_horizontal_edges() {
        let edges = edges_for(Mode::Subtitle);
        assert_eq!(edges, &[ManualEdge::Top, ManualEdge::Bottom]);
        assert!(edges.iter().all(|edge| !edge.is_columns()));
        assert_eq!(edges_for(Mode::Layout).len(), 4);
    }

    /// 一直往一个方向拖，最多只能收到 15%，再拖也不动。
    #[test]
    fn each_edge_stops_at_fifteen_percent() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        // 拖上边一直往下：最多留 15% 高
        let next = shift_edge(start, ManualEdge::Top, 10.0).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖下边一直往上
        let next = shift_edge(start, ManualEdge::Bottom, -10.0).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖左边一直往右
        let next = shift_edge(start, ManualEdge::Left, 10.0).unwrap();
        assert!((next.right - next.left - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖右边一直往左
        let next = shift_edge(start, ManualEdge::Right, -10.0).unwrap();
        assert!((next.right - next.left - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
    }

    /// 到了最小保留范围之后继续拖，结果和上一帧完全一样（调用方据此不记撤销）。
    #[test]
    fn dragging_past_the_floor_changes_nothing() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let at_floor = shift_edge(start, ManualEdge::Top, 10.0).unwrap();
        let further = shift_edge(at_floor, ManualEdge::Top, 5.0).unwrap();
        assert_eq!(at_floor, further);
        // 已经收到最小范围时，只允许往外放，往外放能正常变宽
        let loosened = shift_edge(at_floor, ManualEdge::Top, -0.1).unwrap();
        assert!(loosened.top < at_floor.top, "往外拖应该能放大保留范围");
    }

    /// 另一条边贴在边界上时，面积下限会把本方向撑得比 15% 更宽。
    #[test]
    fn area_floor_widens_the_axis_when_the_other_one_is_thin() {
        // 左右只剩 10%（比 15% 还窄的旧数据）：上下至少留 2.25% / 10% = 22.5%
        let start = band(0.45, 0.55, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 10.0).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "面积下限应把高度撑到 22.5%，实际 {}",
            next.bottom - next.top
        );
        let next = shift_edge(start, ManualEdge::Bottom, -10.0).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5);
    }

    /// 已经比最小保留范围还窄的自动裁剪带：往里拖不动，只能往外放。
    #[test]
    fn a_band_thinner_than_the_floor_can_only_grow() {
        let start = band(0.0, 1.0, 0.0, 0.08);
        // 往里收：保持原样，不许「跳」到 15%
        let next = shift_edge(start, ManualEdge::Bottom, -0.05).unwrap();
        assert_eq!(next, start);
        // 往外放：正常变宽
        let next = shift_edge(start, ManualEdge::Bottom, 0.2).unwrap();
        assert!((next.bottom - 0.28).abs() < 1e-6);
    }

    /// 一次拖动只算被拖的那张图，邻图的保留带不参与计算。
    #[test]
    fn drag_bands_only_touches_the_listed_image() {
        let first = band(0.0, 1.0, 0.0, 1.0);
        let bands = drag_bands(&[(7, first)], ManualEdge::Bottom, -0.3);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].0, 7);
        assert!((bands[0].1.bottom - 0.7).abs() < 1e-6);
        // 拖不动时也照样返回这张图的保留带（值和原来一样），
        // 由调用方比对新旧值来判断这一拖有没有产生变化。
        let stuck = drag_bands(&[(7, first)], ManualEdge::Bottom, 0.0);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].1, first);
    }

    /// 需求 2：整张图已经完整显示时，四条边朝外都没有余量。
    #[test]
    fn a_whole_image_has_no_room_outwards_on_any_edge() {
        let full = band(0.0, 1.0, 0.0, 1.0);
        let targets = [(7, full)];
        for edge in [
            ManualEdge::Top,
            ManualEdge::Bottom,
            ManualEdge::Left,
            ManualEdge::Right,
        ] {
            assert!(
                edge_is_saturated(&targets, edge),
                "整张图在 {edge:?} 方向应算顶住"
            );
        }
    }

    /// 需求 2：图片还有裁切时，朝外那条边还有余量，不能算顶住。
    #[test]
    fn a_cropped_edge_still_has_room_outwards() {
        // 上边被裁到 30%：往上还有余量；下边贴到 1.0，往下顶住了。
        let cropped = band(0.0, 1.0, 0.3, 1.0);
        let targets = [(7, cropped)];
        assert!(
            edge_is_saturated(&targets, ManualEdge::Bottom),
            "下边贴边应算顶住"
        );
        assert!(
            !edge_is_saturated(&targets, ManualEdge::Top),
            "上边应还有余量"
        );
        // 左边被裁到 20%：往左还有余量；右边贴边顶住。
        let columns = band(0.2, 1.0, 0.0, 1.0);
        let targets = [(7, columns)];
        assert!(
            edge_is_saturated(&targets, ManualEdge::Right),
            "右边贴边应算顶住"
        );
        assert!(
            !edge_is_saturated(&targets, ManualEdge::Left),
            "左边应还有余量"
        );
    }

    /// 需求 2：已经收到最小保留范围的边，朝外仍有余量，不能算顶住。
    #[test]
    fn an_edge_at_the_minimum_still_has_room_outwards() {
        let at_floor = shift_edge(band(0.0, 1.0, 0.0, 1.0), ManualEdge::Top, 10.0).unwrap();
        assert!(!edge_is_saturated(&[(7, at_floor)], ManualEdge::Top));
        assert!(edge_is_saturated(&[(7, at_floor)], ManualEdge::Bottom));
    }

    /// 需求 2：整组一起拖时，「顶住」要按整组里所有图一起判。
    ///
    /// 只要还有一张图朝外有余量，这一拖就会改到它，不算顶住，该走正常流程；
    /// 全部顶住才是真的什么都没发生。
    #[test]
    fn a_group_counts_as_saturated_only_when_every_image_is() {
        let full = band(0.0, 1.0, 0.0, 1.0);
        let cropped = band(0.0, 1.0, 0.0, 0.8);
        // 一张贴边、一张还有余量：不顶住，往下拖会改到后者。
        assert!(!edge_is_saturated(
            &[(1, full), (2, cropped)],
            ManualEdge::Bottom
        ));
        // 两张都贴边：顶住，整组都不会变。
        assert!(edge_is_saturated(
            &[(1, full), (2, full)],
            ManualEdge::Bottom
        ));
        // 反方向：两张上边都贴到 0，往上也整组顶住。
        assert!(edge_is_saturated(&[(1, full), (2, full)], ManualEdge::Top));
        // 一张上边还有余量（被裁到 30%），往上不顶住。
        assert!(!edge_is_saturated(
            &[(1, full), (2, band(0.0, 1.0, 0.3, 1.0))],
            ManualEdge::Top
        ));
        // 上下边一起看：往下整组都已经贴到 1.0，顶住；往上只要有一张没到 0，就不顶住。
        let both_at_bottom = band(0.0, 1.0, 0.2, 1.0);
        assert!(edge_is_saturated(
            &[(1, both_at_bottom), (2, both_at_bottom)],
            ManualEdge::Bottom
        ));
        assert!(!edge_is_saturated(
            &[(1, both_at_bottom), (2, both_at_bottom)],
            ManualEdge::Top
        ));
    }

    #[test]
    fn group_follows_only_on_the_shared_axis() {
        // 横排共用高度：改上下边整排一起变，改左右边只有自己变
        assert!(group_follows(
            Mode::Layout,
            LayoutTarget::Row,
            CropAxis::Rows
        ));
        assert!(!group_follows(
            Mode::Layout,
            LayoutTarget::Row,
            CropAxis::Columns
        ));
        // 竖排共用宽度：反过来
        assert!(group_follows(
            Mode::Layout,
            LayoutTarget::Column,
            CropAxis::Columns
        ));
        assert!(!group_follows(
            Mode::Layout,
            LayoutTarget::Column,
            CropAxis::Rows
        ));
        // 比例模式各管各的
        assert!(!group_follows(
            Mode::Layout,
            LayoutTarget::Aspect(1.0),
            CropAxis::Rows
        ));
        // 字幕模式一条一条改
        assert!(!group_follows(
            Mode::Subtitle,
            LayoutTarget::Row,
            CropAxis::Rows
        ));
    }

    #[test]
    fn horizontal_layout_shared_height_drag_keeps_widths() {
        // 横排 1920×1080 与 1000×1000，显示 1920×1080 与 1080×1080，共用高度 1080。
        // 拖 1920 那张的下边往上 180：归一化 -180/1080 = -0.1667。
        let big = band(0.0, 1.0, 0.0, 1.0);
        let small = band(0.0, 1.0, 0.0, 1.0);
        let next_big = shift_edge(big, ManualEdge::Bottom, -180.0 / 1080.0).unwrap();
        let next_small = shift_edge(small, ManualEdge::Bottom, -180.0 / 1080.0).unwrap();
        // 裁剪后两张都是 900 高，按比例给宽：1920 和 1080，宽度一个没变
        let big_cropped = (1920.0, 1080.0 * next_big.bottom);
        let small_cropped = (1000.0, 1000.0 * next_small.bottom);
        assert!((big_cropped.1 - 900.0).abs() < 1e-3);
        assert!((small_cropped.1 - 833.0).abs() < 1.0);
        let shared = big_cropped.1.max(small_cropped.1);
        assert!((shared - 900.0).abs() < 1e-3);
        let big_width = shared * big_cropped.0 / big_cropped.1;
        let small_width = shared * small_cropped.0 / small_cropped.1;
        assert!((big_width - 1920.0).abs() < 1e-3);
        assert!((small_width - 1080.0).abs() < 1.0);
    }
}
