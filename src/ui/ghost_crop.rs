//! 原地裁剪辅助：磁吸档位计算 + 裁剪线和磁吸刻度的绘制。
//!
//! 这里不碰应用状态，只做两件事：
//! 1. 算出某张图在拖上边线或下边线时，附近有哪些可以吸附的档位（单行字幕 / 双行字幕 / 整张画面）。
//! 2. 画出裁剪线和磁吸刻度，让用户看清线落在图片的哪个位置。
//!
//! 曾经这里还负责把被裁掉的整张原图画成半透明预览层，用户明确要求拖边时不要
//! 那个影子（整张原图垫在下面会挡住对裁剪位置的判断），已经删掉。

use gpui::prelude::*;
use gpui::{Div, IntoElement, div, px};

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
    // 吸附档位来自字幕块的上下边界，左右两条边没有可吸附的目标，自由拖动。
    if edge.is_columns() {
        return Vec::new();
    }
    let mut result = Vec::new();

    // 整图端点永远存在：拉到 0% / 100% 就是保留完整画面。
    result.push(SnapCandidate {
        ratio: match edge {
            ManualEdge::Top => 0.0,
            _ => 1.0,
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
    if let Some(band) = crop::band_for_block(block, false, bleed, item.size()) {
        let ratio = match edge {
            ManualEdge::Top => band.y0 as f32 / height,
            _ => band.y1 as f32 / height,
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
        && let Some(band) = crop::band_for_block(block, true, bleed, item.size())
    {
        let ratio = match edge {
            ManualEdge::Top => band.y0 as f32 / height,
            _ => band.y1 as f32 / height,
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

/// 拖动起始时的保留范围，用来把被裁掉的区域钉在原地。
///
/// 没有拖动时传当前保留范围，这样悬停透视和拖动透视用的是同一套算法。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostAnchor {
    pub x0: u32,
    pub y0: u32,
}

impl GhostAnchor {
    pub fn of_band(band: CropBand) -> Self {
        GhostAnchor {
            x0: band.x0,
            y0: band.y0,
        }
    }
}

/// 拖动裁剪线时，在可吸附的位置画一排淡淡的虚刻度。只有上下两条边有刻度。
pub fn render_snap_ticks(
    item: &Item,
    edge: ManualEdge,
    anchor_y0: u32,
    display_width: f32,
    scale_y: f32,
    bleed: f32,
    theme: Theme,
) -> impl IntoElement {
    let mut layer = div()
        .debug_selector(|| "snap-ticks".to_string())
        .absolute()
        .inset_0();
    for candidate in candidates(item, edge, bleed) {
        let y = (candidate.ratio * item.height as f32 - anchor_y0 as f32) * scale_y;
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

/// 裁剪边缘提示要画在哪里。
pub struct CropLineView {
    pub band: CropBand,
    /// 正在拖的那条边；`None` 表示只是指针停在图块上，还没有开始拖。
    ///
    /// 拖动时只有这一条边突出显示，另外三条退成很淡的细线；没有拖动时
    /// 四条边一视同仁，都是轻微高亮。
    pub dragging: Option<ManualEdge>,
    pub anchor: GhostAnchor,
    /// 显示像素 / 原图像素，横向和纵向各一份
    pub scale: (f32, f32),
}


/// 画裁剪边缘提示。
///
/// 只突出正在调的那条边：它是一条细的圆角短条，两头收圆，长度取图宽的六成
/// 但不超过 `CROP_HINT_LEN`，居中放；另外三条边只留一根很淡的细线做参照。
/// 这样一眼能看出在调哪条边，画面也不会被四条粗线铺满。
///
/// `fade` 是这段提示的显现进度：指针刚靠近图块时从 0 涨到 1（约 120 毫秒），
/// 松手后从 1 退回默认的半透明状态（约 500 毫秒）。取值为 0 到 1。
pub fn render_crop_lines(
    view: CropLineView,
    theme: Theme,
    snapped: Option<SnapTier>,
    language: Language,
    fade: f32,
) -> Div {
    let CropLineView {
        band,
        dragging,
        anchor,
        scale,
    } = view;
    let (scale_x, scale_y) = scale;
    // 线相对于锚点的位置：拖动时锚点固定，线跟着手指走。卡片原地不动，
    // 线也就直接压在裁剪带上，不需要再补一段位移。
    let row_top = (band.y0 as f32 - anchor.y0 as f32) * scale_y;
    let row_bottom = (band.y1 as f32 - anchor.y0 as f32) * scale_y;
    let column_left = (band.x0 as f32 - anchor.x0 as f32) * scale_x;
    let column_right = (band.x1 as f32 - anchor.x0 as f32) * scale_x;
    let full_width = (column_right - column_left).max(1.0);
    let full_height = (row_bottom - row_top).max(1.0);

    // 显现进度越低，颜色越淡。四条边共用这个进度，一起淡进淡出。
    let fade = fade.clamp(0.0, 1.0);
    // 拖动中的那条边最显眼；没有拖动时四条边都是轻微高亮。
    let active_strength = tokens::CROP_HINT_REST + (1.0 - tokens::CROP_HINT_REST) * fade;
    let rest_strength = tokens::CROP_HINT_REST * fade;
    let active_color = theme.accent.opacity(active_strength);
    let rest_color = theme.accent.opacity(rest_strength);

    // 短圆角条的长度：图窄的时候跟着变窄，但不小于 24 像素，保证还看得见。
    let bar_len = |full: f32| {
        (full * 0.6)
            .min(f32::from(size::CROP_HINT_LEN))
            .max(24.0_f32.min(full))
    };

    let label = move |tier: SnapTier| {
        div()
            .debug_selector(|| "crop-snap-label".to_string())
            .px(px(8.))
            .py(px(3.))
            .rounded(px(tokens::CROP_BAR_RADIUS))
            .bg(theme.accent.opacity(active_strength))
            .text_color(theme.accent_text)
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .child(tier.label(language))
    };

    // 一条横边：正在拖它的时候画短圆角条，否则只画一根细线。
    let row_line = move |y: f32, with_label: Option<SnapTier>, active: bool, edge_name: &str| {
        let thickness = if active {
            size::CROP_HINT_BAR
        } else {
            size::CROP_GHOST_LINE
        };
        let mut line = div()
            .debug_selector({
                let edge_name = edge_name.to_owned();
                move || format!("crop-line-{edge_name}")
            })
            .absolute()
            .left(px(0.0))
            .top(px(y - f32::from(thickness) / 2.0))
            .w(px(full_width))
            .h(thickness)
            .bg(if active { active_color } else { rest_color });
        if active {
            // 短圆角条居中放在这条边上，两头收圆。
            let bar = bar_len(full_width);
            line = line.child(
                div()
                    .debug_selector(|| format!("crop-bar-{edge_name}"))
                    .absolute()
                    .left(px((full_width - bar) / 2.0))
                    .top(px(0.))
                    .w(px(bar))
                    .h(thickness)
                    .rounded(px(tokens::CROP_BAR_RADIUS))
                    .bg(active_color),
            );
        }
        line.when_some(with_label, |this, tier| {
            this.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(-30.))
                    .w(px(full_width))
                    .flex()
                    .justify_center()
                    .child(label(tier)),
            )
        })
    };
    // 一条竖边：同样的规矩，方向转过来。
    let column_line =
        move |x: f32, with_label: Option<SnapTier>, active: bool, edge_name: &str| {
            let thickness = if active {
                size::CROP_HINT_BAR
            } else {
                size::CROP_GHOST_LINE
            };
            let mut line = div()
                .debug_selector({
                    let edge_name = edge_name.to_owned();
                    move || format!("crop-line-{edge_name}")
                })
                .absolute()
                .left(px(x - f32::from(thickness) / 2.0))
                .top(px(row_top))
                .w(thickness)
                .h(px(full_height))
                .bg(if active { active_color } else { rest_color });
            if active {
                let bar = bar_len(full_height);
                line = line.child(
                    div()
                        .debug_selector(|| format!("crop-bar-{edge_name}"))
                        .absolute()
                        .left(px(0.))
                        .top(px((full_height - bar) / 2.0))
                        .w(thickness)
                        .h(px(bar))
                        .rounded(radius::PILL)
                        .bg(active_color),
                );
            }
            line.when_some(with_label, |this, tier| {
                this.child(
                    div()
                        .absolute()
                        .left(px(12.))
                        .top(px(0.))
                        .h(px(full_height))
                        .flex()
                        .items_center()
                        .child(label(tier)),
                )
            })
        };

    // 吸附档位标签、以及「只有这条边突出」的判断，都只看正在拖的那条边。
    // 没有拖动时，四条边一视同仁，谁也不算正在调。
    let is_active = |target: ManualEdge| dragging == Some(target);
    let label_for = |target: ManualEdge| {
        if is_active(target) { snapped } else { None }
    };
    div()
        .absolute()
        .inset_0()
        .child(row_line(
            row_top,
            label_for(ManualEdge::Top),
            is_active(ManualEdge::Top),
            "top",
        ))
        .child(row_line(
            row_bottom,
            label_for(ManualEdge::Bottom),
            is_active(ManualEdge::Bottom),
            "bottom",
        ))
        .when(edge_is_columns(dragging), |this| {
            this.child(column_line(
                column_left,
                label_for(ManualEdge::Left),
                is_active(ManualEdge::Left),
                "left",
            ))
            .child(column_line(
                column_right,
                label_for(ManualEdge::Right),
                is_active(ManualEdge::Right),
                "right",
            ))
        })
}

/// 要不要画左右两条竖边。
///
/// 排版模式下四条边都能调，所以始终画；字幕模式只能调上下两条，不画竖边。
/// 没有在拖动的时候按当前模式判断，拖动中按正在拖的那条边判断。
fn edge_is_columns(dragging: Option<ManualEdge>) -> bool {
    dragging.is_none_or(ManualEdge::is_columns)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineBox;
    use image::RgbImage;
    use std::sync::Arc;

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
            source: crate::model::SourceImage::Pixels(Arc::new(RgbImage::new(1, 1))),
            block: Some(crate::model::SubtitleBlock { lines }),
            status: ItemStatus::Ready,
            keep_whole: false,
            manual: None,
            analysis: Default::default(),
            fingerprint: Default::default(),
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
        let band =
            crop::band_for_block(item.block.as_ref().unwrap(), false, 0.25, (1000, 1000)).unwrap();
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
        let band =
            crop::band_for_block(item.block.as_ref().unwrap(), false, 0.25, (1000, 1000)).unwrap();
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

        let single = crop::band_for_block(block, false, 0.25, (1000, 1000)).unwrap();
        let double = crop::band_for_block(block, true, 0.25, (1000, 1000)).unwrap();
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
    fn candidates_are_sorted_and_unique() {
        let item = item_with_lines(1000, vec![line(0.0, 60.0), line(80.0, 140.0)]);
        let candidates = candidates(&item, ManualEdge::Top, 0.25);
        let ratios: Vec<f32> = candidates.iter().map(|c| c.ratio).collect();
        assert!(ratios.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(candidates.len(), 1);
    }
}
