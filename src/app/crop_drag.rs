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
pub fn edge_is_saturated(targets: &[(ItemId, ManualBand)], edge: ManualEdge, mode: Mode) -> bool {
    let outward = match edge {
        ManualEdge::Top | ManualEdge::Left => -1.0,
        ManualEdge::Bottom | ManualEdge::Right => 1.0,
    };
    let probe = drag_bands(targets, edge, outward * OUTWARD_PROBE, mode);
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
    mode: Mode,
) -> Vec<(ItemId, ManualBand)> {
    targets
        .iter()
        .filter_map(|(id, start)| shift_edge(*start, edge, delta, mode).map(|band| (*id, band)))
        .collect()
}

/// 拖这一个方向的边时，这个方向至少要留下多少跨度。
///
/// 排版模式两个下限取更严的那个：本方向留 15%，同时整个保留范围的面积不低于
/// 2.25%。另一个方向已经很窄时，光靠 15% 撑不住面积，这个方向就得留得更宽。
///
/// 字幕模式只用 2% 这一条，不套面积下限：字幕模式左右两条边不能拖，横向范围
/// 是自动识别留下的，横排的窄带在横向上可能本来就很窄，拿面积去除会把纵向的
/// 最小高度撑得比自动识别范围还大。左右两条边在字幕模式本来不存在，万一被调用
/// 也走同一条简单下限，保证两条边照样不会交叉。
fn min_span(mode: Mode, other: f32) -> f32 {
    match mode {
        Mode::Subtitle => ManualBand::SUBTITLE_MIN_KEEP_RATIO,
        Mode::Layout => ManualBand::MIN_KEEP_RATIO.max(ManualBand::MIN_KEEP_AREA / other.max(1e-6)),
    }
}

/// 这张图的这条边被拖动之后的新保留带。
///
/// `delta` 是归一化后的拖动量。四条边各自独立：拖上下边不动左右，拖左右边不动上下。
/// 已经贴着图片边界、或已经收到最小保留范围时，继续往同一个方向拖结果不变，
/// 调用方据此判定「这次拖动没产生任何变化」，不记撤销也不改数据。
pub fn shift_edge(
    start: ManualBand,
    edge: ManualEdge,
    delta: f32,
    mode: Mode,
) -> Option<ManualBand> {
    if !delta.is_finite() {
        return None;
    }
    let (left, right, top, bottom) = (start.left, start.right, start.top, start.bottom);
    let min_height = min_span(mode, right - left);
    let min_width = min_span(mode, bottom - top);
    // 已经存在的裁剪带可能比最小保留范围还窄（旧数据、其它来源导入的数据都可能
    // 有），这种情况下只许往外放、不许再往里收，否则用户一拖就会「跳」到本模式
    // 的最小保留范围。
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

/// 按住 Shift 精细调边时，把屏幕位移换算成「占整张原图」的归一化比例增量。
///
/// `image_px` 是原图在这条轴上的像素数，`axis_scale` 是这张图当前的显示缩放
/// （1 个原图像素占几个屏幕像素），`factor` 是精细调节的减速倍数：倍数越大，
/// 同样的屏幕位移换来的比例越小。调用方保证 `factor` 在 [1.0, 8.0]；传进来
/// 不是有限正数时按 1.0 处理，也就是不减速。
///
/// 分母最低按 1.0 算，和 `drag_crop_handle` 里 `(image_px * axis_scale).max(1.0)`
/// 的写法一致：图还没加载出来、缩放还是 0 的时候不会除出无穷大。
pub fn drag_ratio(drag_px: f32, image_px: f32, axis_scale: f32, factor: f32) -> f32 {
    if !drag_px.is_finite() {
        return 0.0;
    }
    let factor = if factor.is_finite() && factor > 0.0 {
        factor
    } else {
        1.0
    };
    drag_px / (image_px * axis_scale * factor).max(1.0)
}

/// 把一个归一化比例对齐到最近的原图整像素。
///
/// 精细调边时一次只挪几分之一个原图像素，比例会落在半个像素上；直接拿去算
/// 裁剪范围，导出时再取整就容易跳一格。这里按 `pixels`（这条轴上的原图像素
/// 数）取最近的整数像素。`pixels` 为 0（图还没加载出来）或比例不是有限值时
/// 原样返回。
///
/// 这里不夹紧到 0..1，夹紧由 `ManualBand::clamped` 负责。
pub fn quantize_ratio(ratio: f32, pixels: u32) -> f32 {
    if !ratio.is_finite() || pixels == 0 {
        return ratio;
    }
    let pixels = pixels as f32;
    (ratio * pixels).round() / pixels
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
        let next = shift_edge(start, ManualEdge::Bottom, -0.2, Mode::Layout).unwrap();
        assert_eq!(next.top, 0.0, "拖下边缘不能动上边缘");
        assert!((next.bottom - 0.8).abs() < 1e-6);
    }

    #[test]
    fn top_edge_keeps_bottom_and_crops_top() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 0.2, Mode::Layout).unwrap();
        assert!((next.top - 0.2).abs() < 1e-6);
        assert_eq!(next.bottom, 1.0, "拖上边缘不能动下边缘");
    }

    #[test]
    fn right_edge_keeps_left_and_crops_right() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Right, -0.25, Mode::Layout).unwrap();
        assert!((next.right - 0.75).abs() < 1e-6);
        assert_eq!(next.left, 0.0);
        // 横向拖动不能动纵向范围
        assert_eq!(next.top, 0.0);
        assert_eq!(next.bottom, 1.0);
    }

    #[test]
    fn left_edge_keeps_right_and_crops_left() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Left, 0.25, Mode::Layout).unwrap();
        assert!((next.left - 0.25).abs() < 1e-6);
        assert_eq!(next.right, 1.0);
    }

    #[test]
    fn dragging_outwards_when_whole_image_does_nothing() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        // 已经整张画面，继续往外拖不应该产生任何变化
        assert_eq!(
            shift_edge(start, ManualEdge::Bottom, 0.5, Mode::Layout),
            Some(start)
        );
        assert_eq!(
            shift_edge(start, ManualEdge::Top, -0.5, Mode::Layout),
            Some(start)
        );
        assert_eq!(
            shift_edge(start, ManualEdge::Right, 0.5, Mode::Layout),
            Some(start)
        );
        assert_eq!(
            shift_edge(start, ManualEdge::Left, -0.5, Mode::Layout),
            Some(start)
        );
    }

    /// 一直往一条边的方向拖，两条边也不会交叉。
    ///
    /// 本方向最少留 15%；另一方向已经很窄时，按面积下限算出来的跨度更大，
    /// 断言用的是这个更大的下限，不是写死的 15%。
    #[test]
    fn edges_cannot_cross_each_other() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 5.0, Mode::Layout).unwrap();
        assert!(
            next.bottom - next.top >= ManualBand::MIN_KEEP_RATIO - 1e-5,
            "上下边不能交叉，实际留下 {}",
            next.bottom - next.top
        );
        assert!(next.bottom > next.top, "上下边不能交叉");
        let next = shift_edge(start, ManualEdge::Left, 5.0, Mode::Layout).unwrap();
        assert!(
            next.right - next.left >= ManualBand::MIN_KEEP_RATIO - 1e-5,
            "左右边不能交叉，实际留下 {}",
            next.right - next.left
        );
        assert!(next.right > next.left, "左右边不能交叉");

        // 左右只剩 10%：纵向至少留 2.25% / 10% = 22.5%，否则两条边会挤到一起。
        let thin_columns = band(0.45, 0.55, 0.0, 1.0);
        let next = shift_edge(thin_columns, ManualEdge::Top, 5.0, Mode::Layout).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "另一方向只剩 10% 时，上下边应留到 22.5%，实际 {}",
            next.bottom - next.top
        );
        // 反过来：上下只剩 10%，左右边同样要留到 22.5%。
        let thin_rows = band(0.0, 1.0, 0.45, 0.55);
        let next = shift_edge(thin_rows, ManualEdge::Left, 5.0, Mode::Layout).unwrap();
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

    /// 排版模式一直往一个方向拖，最多只能收到 15%，再拖也不动。
    #[test]
    fn each_edge_stops_at_fifteen_percent() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        // 拖上边一直往下：最多留 15% 高
        let next = shift_edge(start, ManualEdge::Top, 10.0, Mode::Layout).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖下边一直往上
        let next = shift_edge(start, ManualEdge::Bottom, -10.0, Mode::Layout).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖左边一直往右
        let next = shift_edge(start, ManualEdge::Left, 10.0, Mode::Layout).unwrap();
        assert!((next.right - next.left - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
        // 拖右边一直往左
        let next = shift_edge(start, ManualEdge::Right, -10.0, Mode::Layout).unwrap();
        assert!((next.right - next.left - ManualBand::MIN_KEEP_RATIO).abs() < 1e-6);
    }

    /// 到了最小保留范围之后继续拖，结果和上一帧完全一样（调用方据此不记撤销）。
    #[test]
    fn dragging_past_the_floor_changes_nothing() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let at_floor = shift_edge(start, ManualEdge::Top, 10.0, Mode::Layout).unwrap();
        let further = shift_edge(at_floor, ManualEdge::Top, 5.0, Mode::Layout).unwrap();
        assert_eq!(at_floor, further);
        // 已经收到最小范围时，只允许往外放，往外放能正常变宽
        let loosened = shift_edge(at_floor, ManualEdge::Top, -0.1, Mode::Layout).unwrap();
        assert!(loosened.top < at_floor.top, "往外拖应该能放大保留范围");
    }

    /// 另一条边贴在边界上时，面积下限会把本方向撑得比 15% 更宽。
    #[test]
    fn area_floor_widens_the_axis_when_the_other_one_is_thin() {
        // 左右只剩 10%（比 15% 还窄的旧数据）：上下至少留 2.25% / 10% = 22.5%
        let start = band(0.45, 0.55, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 10.0, Mode::Layout).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "面积下限应把高度撑到 22.5%，实际 {}",
            next.bottom - next.top
        );
        let next = shift_edge(start, ManualEdge::Bottom, -10.0, Mode::Layout).unwrap();
        assert!((next.bottom - next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5);
    }

    /// 已经比最小保留范围还窄的自动裁剪带：往里拖不动，只能往外放。
    #[test]
    fn a_band_thinner_than_the_floor_can_only_grow() {
        let start = band(0.0, 1.0, 0.0, 0.08);
        // 往里收：保持原样，不许「跳」到 15%
        let next = shift_edge(start, ManualEdge::Bottom, -0.05, Mode::Layout).unwrap();
        assert_eq!(next, start);
        // 往外放：正常变宽
        let next = shift_edge(start, ManualEdge::Bottom, 0.2, Mode::Layout).unwrap();
        assert!((next.bottom - 0.28).abs() < 1e-6);
    }

    /// 字幕模式只调上下两条边，高度能一直收到 2%，比排版模式的 15% 小得多。
    #[test]
    fn subtitle_mode_can_shrink_the_height_to_two_percent() {
        // 把下限写死成 2% 断言一次：将来谁改这个数，这条测试会拦下来。
        assert!((ManualBand::SUBTITLE_MIN_KEEP_RATIO - 0.02).abs() < 1e-6);
        let start = band(0.0, 1.0, 0.0, 1.0);
        // 拖上边一直往下
        let next = shift_edge(start, ManualEdge::Top, 10.0, Mode::Subtitle).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6,
            "字幕模式高度应收到 2%，实际 {}",
            next.bottom - next.top
        );
        // 拖下边一直往上
        let next = shift_edge(start, ManualEdge::Bottom, -10.0, Mode::Subtitle).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6,
            "字幕模式高度应收到 2%，实际 {}",
            next.bottom - next.top
        );
        // 到了 2% 之后继续拖，结果和上一帧完全一样（调用方据此不记撤销）
        let at_floor = shift_edge(start, ManualEdge::Top, 10.0, Mode::Subtitle).unwrap();
        let further = shift_edge(at_floor, ManualEdge::Top, 5.0, Mode::Subtitle).unwrap();
        assert_eq!(at_floor, further);
    }

    /// 字幕模式不套面积下限：横向很窄时，纵向照样能收到 2%。
    ///
    /// 同一份数据在排版模式会被撑到 2.25% / 10% = 22.5%。字幕模式左右两条边不能拖，
    /// 横向是自动识别留下的，拿面积下限去撑纵向下限，手动能调到的范围就会比自动
    /// 识别出来的字幕带还大，正是用户抱怨的情况。
    #[test]
    fn subtitle_mode_ignores_the_area_floor() {
        let start = band(0.45, 0.55, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 10.0, Mode::Subtitle).unwrap();
        assert!(
            (next.bottom - next.top - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6,
            "字幕模式横向很窄时高度也应收到 2%，实际 {}",
            next.bottom - next.top
        );
        // 排版模式同一份数据仍按面积下限撑到 22.5%，两套下限互不影响。
        let layout_next = shift_edge(start, ManualEdge::Top, 10.0, Mode::Layout).unwrap();
        assert!(
            (layout_next.bottom - layout_next.top - ManualBand::MIN_KEEP_AREA / 0.10).abs() < 1e-5,
            "排版模式仍应撑到 22.5%，实际 {}",
            layout_next.bottom - layout_next.top
        );
    }

    /// 字幕模式下左右两条边本来就不能拖，函数万一被调用也走同一条简单下限：
    /// 2%，不套面积下限，两条边照样不会交叉。
    #[test]
    fn subtitle_mode_horizontal_edges_use_the_same_simple_floor() {
        let start = band(0.0, 1.0, 0.45, 0.55);
        let next = shift_edge(start, ManualEdge::Left, 10.0, Mode::Subtitle).unwrap();
        assert!(
            (next.right - next.left - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6,
            "字幕模式左右边也应收到 2%，实际 {}",
            next.right - next.left
        );
        let next = shift_edge(start, ManualEdge::Right, -10.0, Mode::Subtitle).unwrap();
        assert!((next.right - next.left - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6);
    }

    /// 比字幕模式下限还窄的旧裁剪带：往里拖不动，不许跳到 2%，只能往外放。
    ///
    /// 正常自动识别出来的字幕带至少占原图高度 3%（MIN_LINE_HEIGHT_RATIO），不会
    /// 比 2% 还窄；这里用 1% 的带模拟旧数据、外部导入的数据。
    #[test]
    fn a_subtitle_band_thinner_than_the_floor_can_only_grow() {
        // 旧数据里的裁剪带只有原图高度的 1%，比 2% 的下限还窄。
        let start = band(0.0, 1.0, 0.0, 0.01);
        // 往里收：保持原样，不许「跳」到 2%
        let next = shift_edge(start, ManualEdge::Bottom, -0.05, Mode::Subtitle).unwrap();
        assert_eq!(next, start);
        // 往外放：正常变宽
        let next = shift_edge(start, ManualEdge::Bottom, 0.2, Mode::Subtitle).unwrap();
        assert!((next.bottom - 0.21).abs() < 1e-6);
        // 拖上边同样只许往外放
        let next = shift_edge(start, ManualEdge::Top, -0.1, Mode::Subtitle).unwrap();
        assert_eq!(next, start);
    }

    /// 字幕模式收到 2% 下限后，朝外仍有余量，不能算顶住。
    #[test]
    fn subtitle_mode_at_the_floor_still_has_room_outwards() {
        let at_floor = shift_edge(
            band(0.0, 1.0, 0.0, 1.0),
            ManualEdge::Top,
            10.0,
            Mode::Subtitle,
        )
        .unwrap();
        assert!(
            (at_floor.bottom - at_floor.top - ManualBand::SUBTITLE_MIN_KEEP_RATIO).abs() < 1e-6,
            "字幕模式应收到 2%，实际 {}",
            at_floor.bottom - at_floor.top
        );
        assert!(!edge_is_saturated(
            &[(7, at_floor)],
            ManualEdge::Top,
            Mode::Subtitle
        ));
        assert!(edge_is_saturated(
            &[(7, at_floor)],
            ManualEdge::Bottom,
            Mode::Subtitle
        ));
    }

    /// 一次拖动只算被拖的那张图，邻图的保留带不参与计算。
    #[test]
    fn drag_bands_only_touches_the_listed_image() {
        let first = band(0.0, 1.0, 0.0, 1.0);
        let bands = drag_bands(&[(7, first)], ManualEdge::Bottom, -0.3, Mode::Layout);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].0, 7);
        assert!((bands[0].1.bottom - 0.7).abs() < 1e-6);
        // 拖不动时也照样返回这张图的保留带（值和原来一样），
        // 由调用方比对新旧值来判断这一拖有没有产生变化。
        let stuck = drag_bands(&[(7, first)], ManualEdge::Bottom, 0.0, Mode::Layout);
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
                edge_is_saturated(&targets, edge, Mode::Layout),
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
            edge_is_saturated(&targets, ManualEdge::Bottom, Mode::Layout),
            "下边贴边应算顶住"
        );
        assert!(
            !edge_is_saturated(&targets, ManualEdge::Top, Mode::Layout),
            "上边应还有余量"
        );
        // 左边被裁到 20%：往左还有余量；右边贴边顶住。
        let columns = band(0.2, 1.0, 0.0, 1.0);
        let targets = [(7, columns)];
        assert!(
            edge_is_saturated(&targets, ManualEdge::Right, Mode::Layout),
            "右边贴边应算顶住"
        );
        assert!(
            !edge_is_saturated(&targets, ManualEdge::Left, Mode::Layout),
            "左边应还有余量"
        );
    }

    /// 需求 2：已经收到最小保留范围的边，朝外仍有余量，不能算顶住。
    #[test]
    fn an_edge_at_the_minimum_still_has_room_outwards() {
        let at_floor = shift_edge(
            band(0.0, 1.0, 0.0, 1.0),
            ManualEdge::Top,
            10.0,
            Mode::Layout,
        )
        .unwrap();
        assert!(!edge_is_saturated(
            &[(7, at_floor)],
            ManualEdge::Top,
            Mode::Layout
        ));
        assert!(edge_is_saturated(
            &[(7, at_floor)],
            ManualEdge::Bottom,
            Mode::Layout
        ));
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
            ManualEdge::Bottom,
            Mode::Layout
        ));
        // 两张都贴边：顶住，整组都不会变。
        assert!(edge_is_saturated(
            &[(1, full), (2, full)],
            ManualEdge::Bottom,
            Mode::Layout
        ));
        // 反方向：两张上边都贴到 0，往上也整组顶住。
        assert!(edge_is_saturated(
            &[(1, full), (2, full)],
            ManualEdge::Top,
            Mode::Layout
        ));
        // 一张上边还有余量（被裁到 30%），往上不顶住。
        assert!(!edge_is_saturated(
            &[(1, full), (2, band(0.0, 1.0, 0.3, 1.0))],
            ManualEdge::Top,
            Mode::Layout
        ));
        // 上下边一起看：往下整组都已经贴到 1.0，顶住；往上只要有一张没到 0，就不顶住。
        let both_at_bottom = band(0.0, 1.0, 0.2, 1.0);
        assert!(edge_is_saturated(
            &[(1, both_at_bottom), (2, both_at_bottom)],
            ManualEdge::Bottom,
            Mode::Layout
        ));
        assert!(!edge_is_saturated(
            &[(1, both_at_bottom), (2, both_at_bottom)],
            ManualEdge::Top,
            Mode::Layout
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
        let next_big = shift_edge(big, ManualEdge::Bottom, -180.0 / 1080.0, Mode::Layout).unwrap();
        let next_small =
            shift_edge(small, ManualEdge::Bottom, -180.0 / 1080.0, Mode::Layout).unwrap();
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

    #[test]
    fn fine_drag_is_eight_times_slower_at_factor_eight() {
        let plain = drag_ratio(40.0, 1080.0, 0.4, 1.0);
        let fine = drag_ratio(40.0, 1080.0, 0.4, 8.0);
        assert_eq!(fine * 8.0, plain, "八倍减速换来的增量应正好是不减速的 1/8");
        // 40 屏幕像素 ÷（1080 × 0.4 × 8）= 0.01157…，占整图高度的一百分之一多一点。
        assert!((fine - 40.0 / (1080.0 * 0.4 * 8.0)).abs() < 1e-9);
    }

    #[test]
    fn fine_drag_keeps_the_sign_of_the_pointer_movement() {
        let towards_top = drag_ratio(-24.0, 1080.0, 0.4, 8.0);
        let towards_bottom = drag_ratio(24.0, 1080.0, 0.4, 8.0);
        assert!(
            towards_top < 0.0,
            "往负方向拖应该是负增量，实际 {towards_top}"
        );
        assert_eq!(
            towards_top + towards_bottom,
            0.0,
            "正负两个方向的位移应互相对称"
        );
        assert_eq!(drag_ratio(0.0, 1080.0, 0.4, 8.0), 0.0, "没动就是零增量");
        assert_eq!(
            drag_ratio(f32::NAN, 1080.0, 0.4, 8.0),
            0.0,
            "位移不是有限值时当作没动"
        );
        assert_eq!(
            drag_ratio(f32::INFINITY, 1080.0, 0.4, 8.0),
            0.0,
            "无穷大的位移同样当作没动"
        );
    }

    #[test]
    fn fine_drag_denominator_never_drops_below_one() {
        // 图还没加载出来（原图 0 像素）或还没缩放（0 倍）时，分母按 1.0 算，
        // 增量就是屏幕位移本身，不会除出无穷大。
        assert_eq!(drag_ratio(12.0, 0.0, 0.4, 1.0), 12.0);
        assert_eq!(drag_ratio(12.0, 1080.0, 0.0, 1.0), 12.0);
        assert_eq!(drag_ratio(12.0, 0.0, 0.0, 8.0), 12.0);
        // 尺寸正常时分母远大于 1，走的是真实换算。
        assert!(drag_ratio(12.0, 1080.0, 0.4, 1.0) < 1.0);
    }

    #[test]
    fn fine_drag_treats_a_bad_factor_as_no_slowdown() {
        let plain = drag_ratio(40.0, 1080.0, 0.4, 1.0);
        for bad_factor in [0.0, -4.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                drag_ratio(40.0, 1080.0, 0.4, bad_factor),
                plain,
                "factor = {bad_factor} 应按 1.0 处理"
            );
        }
    }

    #[test]
    fn quantize_ratio_rounds_to_the_nearest_whole_pixel() {
        // 从画面正中（1920 宽是 960 像素、1080 高是 540 像素）再挪不到一个像素：
        // 0.3 和 0.4 个像素往下取整，0.5 正好一半、0.6 往上取整。
        // f32 的 round 对正数按「一半就远离 0」取整。
        let offsets = [0.3_f32, 0.4, 0.5, 0.6];
        for (pixels, expected) in [
            (1920_u32, [960.0_f32, 960.0, 961.0, 961.0]),
            (1080_u32, [540.0_f32, 540.0, 541.0, 541.0]),
        ] {
            for (offset, want_px) in offsets.into_iter().zip(expected) {
                let ratio = 0.5 + offset / pixels as f32;
                let quantized = quantize_ratio(ratio, pixels);
                let landed_px = quantized * pixels as f32;
                assert!(
                    (landed_px - want_px).abs() < 1e-3,
                    "{pixels} 像素时挪 {offset} 个像素应落在第 {want_px} 个像素，实际 {landed_px}"
                );
                assert!(
                    (landed_px - landed_px.round()).abs() < 1e-3,
                    "对齐后乘回像素数必须是整数，实际 {landed_px}"
                );
            }
        }
    }

    #[test]
    fn quantize_ratio_passes_through_missing_size_and_nan() {
        let ratio = 0.312_345_6_f32;
        assert_eq!(quantize_ratio(ratio, 0), ratio, "原图像素数为 0 时原样返回");
        assert!(
            quantize_ratio(f32::NAN, 1080).is_nan(),
            "比例不是有限值时原样返回 NaN"
        );
        assert_eq!(
            quantize_ratio(f32::INFINITY, 1080),
            f32::INFINITY,
            "无穷大的比例同样原样返回"
        );
        // 这里不夹紧到 0..1：超范围的比例照常取整，夹紧由 ManualBand::clamped 负责。
        assert!((quantize_ratio(1.25, 4) - 1.25).abs() < 1e-6);
        assert!((quantize_ratio(-0.3, 10) + 0.3).abs() < 1e-6);
    }

    #[test]
    fn fine_drag_quantized_top_sits_on_a_whole_pixel() {
        // 原图高 1080、屏幕上缩到 0.4 倍（显示高 432 像素）。起点 top = 0.3，
        // 也就是第 324 个原图像素；按住 Shift 往负方向拖 40 个屏幕像素。
        const IMAGE_HEIGHT: u32 = 1080;
        const IMAGE_PX: f32 = IMAGE_HEIGHT as f32;
        const AXIS_SCALE: f32 = 0.4;
        const FINE_FACTOR: f32 = 8.0;
        const START_TOP: f32 = 0.3;

        let delta_ratio = drag_ratio(40.0, IMAGE_PX, AXIS_SCALE, FINE_FACTOR);
        let new_top = quantize_ratio(START_TOP + delta_ratio, IMAGE_HEIGHT);
        let landed_px = new_top * IMAGE_PX;
        let start_px = (START_TOP * IMAGE_PX).round();

        // 任务描述里的增量写法是 40/(1080×0.4×8)，这是「占整图的比例」；
        // 换算成原图像素数要再乘 1080，所以移动的是 12.5 个像素。
        let ratio_from_task = 40.0 / (IMAGE_PX * AXIS_SCALE * FINE_FACTOR);
        assert_eq!(delta_ratio, ratio_from_task, "增量应按描述里的公式算");
        assert_eq!(delta_ratio * IMAGE_PX, 12.5, "换算成原图像素是 12.5 个");

        assert_eq!(start_px, 324.0, "起点 top = 0.3 是第 324 个像素");
        assert_eq!(landed_px, 337.0, "对齐后应落在第 337 个像素");
        assert_eq!(
            (ratio_from_task * IMAGE_PX).round(),
            landed_px - start_px,
            "移动的像素数应等于 round(比例 × 原图高度)"
        );
        assert_eq!(landed_px - start_px, 13.0, "实际挪了 13 个原图像素");
        // 落到整像素上时，比例乘回原图像素数是整数，没有小数尾巴。
        assert_eq!(landed_px.fract(), 0.0);
    }
}
