//! 裁剪线拖动的规则：拖一条边改哪个轴、这一拖要不要带着同一排或同一列的图一起动、
//! 每张图各自的保留带怎么变。
//!
//! 这里只有纯计算，不碰界面、不碰状态，方便单独测试。

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

/// 横排共用高度、竖排共用宽度，所以改共用那条边时整组都要跟着改，
/// 否则只有一张图被裁，排版会把它重新拉满，看上去等于没裁。
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

/// 这张图的这条边被拖动之后的新保留带。
///
/// `delta` 是归一化后的拖动量。四条边各自独立：拖上下边不动左右，拖左右边不动上下。
/// 已经贴着图片边界时继续往外拖，结果和原来一样。
pub fn shift_edge(start: ManualBand, edge: ManualEdge, delta: f32) -> Option<ManualBand> {
    if !delta.is_finite() {
        return None;
    }
    let (left, right, top, bottom) = (start.left, start.right, start.top, start.bottom);
    // 留下比最小宽度再宽一点的余量，避免浮点误差把结果挤到最小宽度以下后又整个丢掉。
    let slack = 0.02;
    match edge {
        ManualEdge::Top => {
            ManualBand::clamped(left, right, (top + delta).min(bottom - slack), bottom)
        }
        ManualEdge::Bottom => {
            ManualBand::clamped(left, right, top, (bottom + delta).max(top + slack))
        }
        ManualEdge::Left => {
            ManualBand::clamped((left + delta).min(right - slack), right, top, bottom)
        }
        ManualEdge::Right => {
            ManualBand::clamped(left, (right + delta).max(left + slack), top, bottom)
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

    #[test]
    fn edges_cannot_cross_each_other() {
        let start = band(0.0, 1.0, 0.0, 1.0);
        let next = shift_edge(start, ManualEdge::Top, 5.0).unwrap();
        assert!(next.bottom - next.top >= 0.01, "上下边不能交叉");
        let next = shift_edge(start, ManualEdge::Left, 5.0).unwrap();
        assert!(next.right - next.left >= 0.01, "左右边不能交叉");
    }

    #[test]
    fn subtitle_mode_has_no_horizontal_edges() {
        let edges = edges_for(Mode::Subtitle);
        assert_eq!(edges, &[ManualEdge::Top, ManualEdge::Bottom]);
        assert!(edges.iter().all(|edge| !edge.is_columns()));
        assert_eq!(edges_for(Mode::Layout).len(), 4);
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
