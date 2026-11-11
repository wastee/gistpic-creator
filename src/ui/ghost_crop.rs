//! 原地幽灵透视裁剪：磁吸档位计算 + 被裁掉区域的半透明原图渲染。
//!
//! 这里不碰应用状态，只做两件事：
//! 1. 算出某张图在拖上边线或下边线时，附近有哪些可以吸附的档位（单行字幕 / 双行字幕 / 整张画面）。
//! 2. 把被裁掉的上半段和下半段原图画成半透明图层，让用户看着上下文拉线。

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Div, IntoElement, ObjectFit, RenderImage, StyledImage, div, img, px};

use crate::app::ManualEdge;
use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand};
use crate::model::{Item, ItemStatus};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, text_size};

/// 磁吸档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapTier {
    /// 单行字幕：第一行字幕的上下沿（含当前出血位）
    SingleLine,
    /// 双行字幕：前两行字幕的合集范围
    TwoLines,
    /// 整张画面：原图最顶端或最底端
    WholeImage,
}

impl SnapTier {
    pub fn key(self) -> &'static str {
        match self {
            SnapTier::SingleLine => "single-line",
            SnapTier::TwoLines => "two-lines",
            SnapTier::WholeImage => "whole-image",
        }
    }

    pub fn label(self, language: Language) -> &'static str {
        match self {
            SnapTier::SingleLine => language.snap_single_line(),
            SnapTier::TwoLines => language.snap_two_lines(),
            SnapTier::WholeImage => language.snap_whole_image(),
        }
    }
}

/// 一个可以吸附的位置。ratio 是按原图高度归一化的纵向坐标（0.0 顶边，1.0 底边）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapCandidate {
    pub ratio: f32,
    pub tier: SnapTier,
}

/// 磁吸结果。tier 为 None 表示没有吸到任何档位，停在手指的位置做自由裁剪。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapResult {
    pub ratio: f32,
    pub tier: Option<SnapTier>,
}

/// 这张图当前可用的吸附档位，按 edge 区分上边线和下边线。
///
/// 没有字幕的图片只有「整张画面」两个端点；只有一行字幕时没有「双行」档。
/// 同一张图里两个档位落到同一位置时只保留一个，避免标签来回闪。
pub fn candidates(item: &Item, edge: ManualEdge, bleed: f32) -> Vec<SnapCandidate> {
    let height = item.height as f32;
    if height <= 0.0 {
        return Vec::new();
    }
    let mut result = Vec::new();

    // 整图端点永远存在：拉到 0% / 100% 就是保留完整画面。
    result.push(SnapCandidate {
        ratio: match edge {
            ManualEdge::Top => 0.0,
            ManualEdge::Bottom => 1.0,
        },
        tier: SnapTier::WholeImage,
    });

    let ready = !matches!(item.status, ItemStatus::Pending | ItemStatus::Failed(_));
    let Some(block) = item.block.as_ref().filter(|_| ready) else {
        return result;
    };
    if block.lines.is_empty() {
        return result;
    }

    // 单行字幕：自动识别到的第一行字幕，上下各带当前出血位。
    if let Some(band) = crop::band_for_block(block, false, bleed, item.height) {
        let ratio = match edge {
            ManualEdge::Top => band.y0 as f32 / height,
            ManualEdge::Bottom => band.y1 as f32 / height,
        };
        let exists = result
            .iter()
            .any(|candidate| (candidate.ratio - ratio).abs() < 1e-6);
        if !exists {
            result.push(SnapCandidate {
                ratio,
                tier: SnapTier::SingleLine,
            });
        }
    }

    // 双行字幕：该图有两行及以上字幕时才提供，吸住前两行的合集。
    if block.lines.len() >= 2
        && let Some(band) = crop::band_for_block(block, true, bleed, item.height)
    {
        let ratio = match edge {
            ManualEdge::Top => band.y0 as f32 / height,
            ManualEdge::Bottom => band.y1 as f32 / height,
        };
        let exists = result
            .iter()
            .any(|candidate| (candidate.ratio - ratio).abs() < 1e-6);
        if !exists {
            result.push(SnapCandidate {
                ratio,
                tier: SnapTier::TwoLines,
            });
        }
    }

    result.sort_by(|a, b| a.ratio.total_cmp(&b.ratio));
    result
}

/// 磁吸：鼠标离某个档位在 SNAP_THRESHOLD_PX 像素内时，把比例吸到档位上。
///
/// display_height 是整张原图在当前画布上的显示高度（屏幕像素）。
/// 8 像素是屏幕上的距离，所以要按显示高度折算成归一化比例。
pub fn snap_crop_ratio(item: &Item, edge: ManualEdge, raw_ratio: f32, bleed: f32) -> SnapResult {
    let display_height = item.height.max(1) as f32;
    snap_crop_ratio_with_display(item, edge, raw_ratio, bleed, display_height)
}

/// 和 [`snap_crop_ratio`] 相同，但按当前画布上的显示高度折算 8 像素磁吸距离。
pub fn snap_crop_ratio_with_display(
    item: &Item,
    edge: ManualEdge,
    raw_ratio: f32,
    bleed: f32,
    display_height: f32,
) -> SnapResult {
    let ratio = raw_ratio.clamp(0.0, 1.0);
    if !display_height.is_finite() || display_height <= 0.0 {
        return SnapResult { ratio, tier: None };
    }
    let threshold = tokens::SNAP_THRESHOLD_PX / display_height;
    let mut best: Option<(f32, SnapCandidate)> = None;
    for candidate in candidates(item, edge, bleed) {
        let distance = (candidate.ratio - ratio).abs();
        if distance <= threshold
            && best
                .as_ref()
                .is_none_or(|(best_distance, _)| distance < *best_distance)
        {
            best = Some((distance, candidate));
        }
    }
    match best {
        Some((_, candidate)) => SnapResult {
            ratio: candidate.ratio,
            tier: Some(candidate.tier),
        },
        None => SnapResult { ratio, tier: None },
    }
}

/// 幽灵图层的几何。全部是相对图块左上角的屏幕像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostLayout {
    /// 上方幽灵层的 top（一般是负值，浮在上一张图上）
    pub top_offset: f32,
    /// 上方幽灵层的高度
    pub top_height: f32,
    /// 下方幽灵层的 top
    pub bottom_offset: f32,
    /// 下方幽灵层的高度
    pub bottom_height: f32,
    /// 整张原图在这个比例下的显示高度
    pub full_height: f32,
}

/// 算幽灵图层：以拖动开始时的保留带为锚点，向上露 0..y0，向下露 y1..原图高度。
///
/// anchor_y0 是拖动开始时保留带的上沿；没有拖动时传当前保留带的上沿，
/// 这样悬停透视和拖动透视用的是同一套算法。
pub fn ghost_layout(
    item: &Item,
    band: CropBand,
    anchor_y0: u32,
    display_width: f32,
) -> GhostLayout {
    let width = item.width.max(1) as f32;
    let scale = display_width / width;
    let full_height = item.height as f32 * scale;
    let anchor = anchor_y0 as f32 * scale;
    GhostLayout {
        top_offset: -anchor,
        top_height: (band.y0 as f32 * scale).max(0.0),
        bottom_offset: (band.y1 as f32 - anchor_y0 as f32) * scale,
        bottom_height: ((item.height.saturating_sub(band.y1)) as f32 * scale).max(0.0),
        full_height,
    }
}

/// 画两段幽灵原图：上方被裁掉的部分和下方被裁掉的部分。
///
/// full_preview 是整张原图的预览图，按 display_width 拉伸后正好铺满
/// ghost_layout 算出来的 full_height，再按位置裁出上下两段。
pub fn render_ghost_layers(
    item: &Item,
    band: CropBand,
    anchor_y0: u32,
    display_width: f32,
    full_preview: Arc<RenderImage>,
    theme: Theme,
) -> Div {
    let layout = ghost_layout(item, band, anchor_y0, display_width);
    let scale = display_width / item.width.max(1) as f32;
    let id = item.id;
    div()
        .debug_selector(move || format!("ghost-{id}"))
        .absolute()
        .inset_0()
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(layout.top_offset))
                .w(px(display_width))
                .h(px(layout.top_height))
                .overflow_hidden()
                .bg(theme.ghost_mask)
                .when(layout.top_height > 0.5, |this| {
                    this.child(
                        img(full_preview.clone())
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .w(px(display_width))
                            .h(px(layout.full_height))
                            .object_fit(ObjectFit::Fill)
                            .opacity(0.30),
                    )
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(layout.bottom_offset))
                .w(px(display_width))
                .h(px(layout.bottom_height))
                .overflow_hidden()
                .bg(theme.ghost_mask)
                .when(layout.bottom_height > 0.5, |this| {
                    this.child(
                        img(full_preview)
                            .absolute()
                            .left(px(0.))
                            .top(px(-(band.y1 as f32 * scale)))
                            .w(px(display_width))
                            .h(px(layout.full_height))
                            .object_fit(ObjectFit::Fill)
                            .opacity(0.30),
                    )
                }),
        )
}

/// 拖动裁剪线时，在可吸附的位置画一排淡淡的虚刻度。
pub fn render_snap_ticks(
    item: &Item,
    edge: ManualEdge,
    anchor_y0: u32,
    display_width: f32,
    scale: f32,
    bleed: f32,
    theme: Theme,
) -> impl IntoElement {
    let mut layer = div()
        .debug_selector(|| "snap-ticks".to_string())
        .absolute()
        .inset_0();
    for candidate in candidates(item, edge, bleed) {
        let y = (candidate.ratio * item.height as f32 - anchor_y0 as f32) * scale;
        layer = layer.child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(y - 0.5))
                .w(px(display_width))
                .h(px(1.))
                .bg(theme.accent.opacity(0.22)),
        );
    }
    layer
}

/// 两条裁剪线和中央的小握把。位置按锚点算，拖动时线跟着手指走。
/// 裁剪线要画在哪里。
pub struct CropLineView {
    pub band: CropBand,
    pub edge: ManualEdge,
    pub anchor_y0: u32,
    pub display_width: f32,
    pub scale: f32,
}

pub fn render_crop_lines(
    view: CropLineView,
    theme: Theme,
    snapped: Option<SnapTier>,
    language: Language,
) -> Div {
    let CropLineView {
        band,
        edge,
        anchor_y0,
        display_width,
        scale,
    } = view;
    let top = (band.y0 as f32 - anchor_y0 as f32) * scale;
    let bottom = (band.y1 as f32 - anchor_y0 as f32) * scale;
    let line = move |y: f32, with_label: Option<SnapTier>, edge_name: &str| {
        div()
            .debug_selector({
                let edge_name = edge_name.to_owned();
                move || format!("crop-line-{edge_name}")
            })
            .absolute()
            .left(px(0.))
            .top(px(y - f32::from(size::CROP_LINE) / 2.0))
            .w(px(display_width))
            .h(size::CROP_LINE)
            .bg(theme.accent)
            .child(
                div()
                    .absolute()
                    .left(px((display_width - f32::from(size::CROP_GRIP_W)) / 2.0))
                    .top(px(-(f32::from(size::CROP_GRIP_H)
                        - f32::from(size::CROP_LINE))
                        / 2.0))
                    .w(size::CROP_GRIP_W)
                    .h(size::CROP_GRIP_H)
                    .rounded(size::CROP_GRIP_H)
                    .bg(theme.accent),
            )
            .when_some(with_label, |this, tier| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(-30.))
                        .w(px(display_width))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .debug_selector(|| "crop-snap-label".to_string())
                                .px(px(8.))
                                .py(px(3.))
                                .rounded(radius::PILL)
                                .bg(theme.accent)
                                .text_color(theme.accent_text)
                                .font_family(tokens::FONT_FAMILY)
                                .text_size(text_size::CAPTION)
                                .child(tier.label(language)),
                        ),
                )
            })
    };
    let top_label = if edge == ManualEdge::Top {
        snapped
    } else {
        None
    };
    let bottom_label = if edge == ManualEdge::Bottom {
        snapped
    } else {
        None
    };
    div()
        .absolute()
        .inset_0()
        .child(line(top, top_label, "top"))
        .child(line(bottom, bottom_label, "bottom"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineBox;
    use image::RgbImage;

    fn line(y0: f32, y1: f32) -> LineBox {
        LineBox {
            x0: 100.0,
            y0,
            x1: 500.0,
            y1,
        }
    }

    fn item_with_lines(height: u32, lines: Vec<LineBox>) -> Item {
        Item {
            id: 1,
            name: "sample.png".to_string(),
            width: 1000,
            height,
            rgb: Arc::new(RgbImage::new(1, 1)),
            block: Some(crate::model::SubtitleBlock { lines }),
            status: ItemStatus::Ready,
            keep_whole: false,
            manual: None,
        }
    }

    fn item_without_subtitle(height: u32) -> Item {
        let mut item = item_with_lines(height, Vec::new());
        item.block = None;
        item.status = ItemStatus::NoSubtitle;
        item.keep_whole = true;
        item
    }

    #[test]
    fn single_line_image_snaps_bottom_to_single_line_tier() {
        // 1000 像素高的图，第一行字幕 800..860，出血位 25% -> 785..875
        let item = item_with_lines(1000, vec![line(800.0, 860.0)]);
        let band = crop::band_for_block(item.block.as_ref().unwrap(), false, 0.25, 1000).unwrap();
        let target = band.y1 as f32 / 1000.0;

        let result =
            snap_crop_ratio_with_display(&item, ManualEdge::Bottom, target + 0.004, 0.25, 1000.0);
        assert_eq!(result.tier, Some(SnapTier::SingleLine));
        assert!((result.ratio - target).abs() < 1e-6, "要吸到字幕下沿");

        let free =
            snap_crop_ratio_with_display(&item, ManualEdge::Bottom, target + 0.1, 0.25, 1000.0);
        assert_eq!(free.tier, None);
        assert!((free.ratio - (target + 0.1)).abs() < 1e-6);
    }

    #[test]
    fn four_argument_api_uses_image_height_as_scale() {
        let item = item_with_lines(1000, vec![line(800.0, 860.0)]);
        let band = crop::band_for_block(item.block.as_ref().unwrap(), false, 0.25, 1000).unwrap();
        let result = snap_crop_ratio(
            &item,
            ManualEdge::Bottom,
            band.y1 as f32 / 1000.0 + 0.004,
            0.25,
        );
        assert_eq!(result.tier, Some(SnapTier::SingleLine));
    }

    #[test]
    fn two_line_image_offers_two_line_tier() {
        let item = item_with_lines(1000, vec![line(700.0, 760.0), line(780.0, 840.0)]);
        let block = item.block.as_ref().unwrap();

        let single = crop::band_for_block(block, false, 0.25, 1000).unwrap();
        let double = crop::band_for_block(block, true, 0.25, 1000).unwrap();
        assert!(double.y1 > single.y1, "双行档要比单行档更低");

        let target = double.y1 as f32 / 1000.0;
        let result =
            snap_crop_ratio_with_display(&item, ManualEdge::Bottom, target - 0.003, 0.25, 1000.0);
        assert_eq!(result.tier, Some(SnapTier::TwoLines));
        assert!((result.ratio - target).abs() < 1e-6);

        let candidates = candidates(&item, ManualEdge::Bottom, 0.25);
        assert!(candidates.iter().any(|c| c.tier == SnapTier::SingleLine));
        assert!(candidates.iter().any(|c| c.tier == SnapTier::TwoLines));
    }

    #[test]
    fn image_without_subtitles_only_snaps_to_whole_image() {
        let item = item_without_subtitle(1000);
        let candidates = candidates(&item, ManualEdge::Bottom, 0.25);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].tier, SnapTier::WholeImage);

        let result = snap_crop_ratio_with_display(&item, ManualEdge::Bottom, 0.995, 0.25, 1000.0);
        assert_eq!(result.tier, Some(SnapTier::WholeImage));
        assert!((result.ratio - 1.0).abs() < 1e-6);

        let free = snap_crop_ratio_with_display(&item, ManualEdge::Top, 0.5, 0.25, 1000.0);
        assert_eq!(free.tier, None);
    }

    #[test]
    fn top_edge_snaps_to_zero_for_whole_image() {
        let item = item_with_lines(1000, vec![line(100.0, 160.0)]);
        let result = snap_crop_ratio_with_display(&item, ManualEdge::Top, 0.002, 0.25, 1000.0);
        assert_eq!(result.tier, Some(SnapTier::WholeImage));
        assert!((result.ratio - 0.0).abs() < 1e-6);
    }

    #[test]
    fn zero_display_height_never_snaps() {
        let item = item_with_lines(1000, vec![line(800.0, 860.0)]);
        let result = snap_crop_ratio_with_display(&item, ManualEdge::Bottom, 0.85, 0.25, 0.0);
        assert_eq!(result.tier, None);
        assert!((result.ratio - 0.85).abs() < 1e-6);
    }

    #[test]
    fn ghost_layout_exposes_cut_away_parts() {
        let item = item_with_lines(1000, vec![line(800.0, 860.0)]);
        let band = CropBand { y0: 785, y1: 875 };
        let layout = ghost_layout(&item, band, 785, 500.0);
        // 500 / 1000 = 0.5 倍
        assert!((layout.top_offset + 392.5).abs() < 1e-3);
        assert!((layout.top_height - 392.5).abs() < 1e-3);
        assert!((layout.bottom_offset - 45.0).abs() < 1e-3);
        assert!((layout.bottom_height - 62.5).abs() < 1e-3);
        assert!((layout.full_height - 500.0).abs() < 1e-3);
    }

    #[test]
    fn ghost_layout_keeps_dragged_band_anchored() {
        let item = item_with_lines(1000, vec![line(800.0, 860.0)]);
        // 拖动中保留带变大，锚点还是拖动开始时的 785
        let band = CropBand { y0: 700, y1: 875 };
        let layout = ghost_layout(&item, band, 785, 500.0);
        assert!((layout.top_height - 350.0).abs() < 1e-3, "上方露到新的 y0");
        assert!(
            (layout.bottom_offset - 45.0).abs() < 1e-3,
            "下方从新的 y1 开始"
        );
    }

    #[test]
    fn ghost_layout_uses_source_coordinates_during_drag() {
        let item = item_with_lines(1000, vec![line(100.0, 160.0)]);
        let band = CropBand { y0: 60, y1: 160 };
        let layout = ghost_layout(&item, band, 0, 500.0);
        assert_eq!(layout.top_offset, 0.0, "拖动时整张原图从原图顶端开始");
        assert!((layout.top_height - 30.0).abs() < 1e-3);
        assert!((layout.bottom_offset - 80.0).abs() < 1e-3);
        assert!((layout.bottom_height - 420.0).abs() < 1e-3);
    }

    #[test]
    fn candidates_are_sorted_and_unique() {
        let item = item_with_lines(1000, vec![line(0.0, 60.0), line(80.0, 140.0)]);
        let candidates = candidates(&item, ManualEdge::Top, 0.25);
        let ratios: Vec<f32> = candidates.iter().map(|c| c.ratio).collect();
        assert!(ratios.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(candidates.len(), 1);
    }
}
