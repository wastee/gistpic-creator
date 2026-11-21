//! 选中卡片的纯几何：圆角怎么过渡、方勾放在哪。
//!
//! 这里不画界面，只把动画进度换成矩形和圆角，方便单独测，也避免和
//! 图块渲染、裁剪线渲染混在一起。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, ObjectFit, div, img, px};

use crate::app::pyramid::PyramidLayer;
use crate::model::ItemId;
use crate::ui::canvas::geometry::Rect;
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::selection;

/// 一张图在当前选中动画进度下，卡片和方勾的几何。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardMetrics {
    /// 选中进度，用来算圆角过渡。
    pub lift: f32,
    /// 卡片矩形，恒等于图块矩形（相对图块左上角）。
    pub card: Rect,
    /// 四个角的圆角：左上、右上、左下、右下。
    pub radii: [f32; 4],
    /// 右上角方勾的矩形。
    pub check: Rect,
    /// 方勾不透明度。
    pub check_opacity: f32,
    /// 变淡遮罩的不透明度；选中图为 0。
    pub dim_alpha: f32,
}

impl CardMetrics {
    /// 按选中、变淡、方勾三个进度算几何。
    ///
    /// `lift` 0 是没选中，1 是刚选中；它现在只驱动圆角过渡，
    /// 不再移动卡片位置。`dim` 0 是不变淡，1 是最终 45%；
    /// `check` 0 是方勾完全消失，1 是完全出现。
    ///
    /// 卡片宽高和位置都恒等于原图块，图片和字幕的像素比例不会跟着变，
    /// 裁剪线和热区也不会因为选中而错位。
    pub fn new(
        frame: Rect,
        base_radii: [f32; 4],
        lift: f32,
        dim: f32,
        check: f32,
    ) -> Self {
        let lift = lift.clamp(0.0, 1.0);
        let dim = dim.clamp(0.0, 1.0);
        let check = check.clamp(0.0, 1.0);
        // 卡片原地不动：用户取消浮动，因为上移会让卡片上边缘和裁剪热区错开，
        // 拖裁剪边时对不准。选中反馈只靠圆角、变淡遮罩和方勾。
        let card = frame;
        let target = f32::from(selection::RADIUS);
        let radii = base_radii.map(|radius| radius + (target - radius) * lift);

        // 方勾从 0.8 倍长到 1 倍，位置始终贴着卡片右上角往里 8px。
        let size = f32::from(selection::CHECK) * (0.8 + 0.2 * check);
        let inset = f32::from(selection::CHECK_INSET);
        let check_rect = Rect::new(
            card.right() - inset - size,
            card.y + inset,
            size,
            size,
        );
        CardMetrics {
            lift,
            card,
            radii,
            check: check_rect,
            check_opacity: check,
            dim_alpha: dim * selection::DIM,
        }
    }
}

/// 没选中的图块上盖一层变淡遮罩。遮罩单独一层，调试选择器方便测不透明度。
pub(crate) fn render_veil(theme: Theme, id: ItemId, frame: Rect, alpha: f32) -> AnyElement {
    div()
        .debug_selector(move || format!("selection-veil-{id}"))
        .absolute()
        .left(px(frame.x))
        .top(px(frame.y))
        .w(px(frame.w))
        .h(px(frame.h))
        .bg(theme.bg.opacity(alpha.clamp(0.0, 1.0)))
        .into_any_element()
}

/// 图块是不是贴着整张图右边/下边，判断用的容差（逻辑像素）。
const EDGE_EPS: f32 = 0.01;

/// 画一张选中卡片：把基础层贴的图块原样铺进卡片，叠变淡遮罩和方勾。
///
/// 卡片和图片块完全重合，所以图块坐标直接沿用，不做任何缩放；纹理也直接复用，
/// 不重新生成。选中反馈只靠圆角、变淡和方勾。
///
/// `layers` 是这一帧源分辨率金字塔的渲染计划，从粗到细排列：最粗的顶层是完整
/// 纹理，先画它垫底，后画的细层盖住它。卡片内容自己铺 `theme.surface` 占位，
/// 计划为空（顶层还没生成）时也不会露出作品底色。
///
/// `offset` 是整张原图左上角相对裁剪带左上角的偏移：图块坐标以整张原图为准，
/// 卡片以裁剪带为准，两者差这一段。`full` 是整张图在屏幕上的逻辑宽高，
/// `edge_cover` 是贴着作品右边/下边时多画的 1 个物理像素。
pub(crate) fn render_card(
    theme: Theme,
    id: ItemId,
    metrics: CardMetrics,
    layers: &[PyramidLayer],
    offset: (f32, f32),
    full: (f32, f32),
    edge_cover: (f32, f32),
) -> AnyElement {
    let card = metrics.card;
    let mut content = div()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .w(px(card.w))
        .h(px(card.h))
        .overflow_hidden()
        .bg(theme.surface)
        .rounded_tl(px(metrics.radii[0]))
        .rounded_tr(px(metrics.radii[1]))
        .rounded_bl(px(metrics.radii[2]))
        .rounded_br(px(metrics.radii[3]));
    // 按渲染计划从粗到细铺：顶层是完整底图，细层只铺已经生成好的块。
    // 贴到整张图右边/下边的图块和基础层一样多画 1 个物理像素，避免取整后
    // 卡片右沿或下沿漏出占位色。
    for layer in layers {
        for tile in &layer.tiles {
            let cover_right = edge_cover.0 > 0.0 && tile.rect.right() >= full.0 - EDGE_EPS;
            let cover_bottom = edge_cover.1 > 0.0 && tile.rect.bottom() >= full.1 - EDGE_EPS;
            let tile_width = tile.rect.w + if cover_right { edge_cover.0 } else { 0.0 };
            let tile_height = tile.rect.h + if cover_bottom { edge_cover.1 } else { 0.0 };
            content = content.child(
                div()
                    .absolute()
                    .left(px(tile.rect.x + offset.0))
                    .top(px(tile.rect.y + offset.1))
                    .w(px(tile_width))
                    .h(px(tile_height))
                    .child(
                        img(tile.image.clone())
                            .w_full()
                            .h_full()
                            .object_fit(ObjectFit::Fill),
                    ),
            );
        }
    }
    if metrics.dim_alpha > 0.001 {
        content = content.child(
            div()
                .debug_selector({
                    let id = id;
                    move || format!("selection-veil-{id}")
                })
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w(px(card.w))
                .h(px(card.h))
                .bg(theme.bg.opacity(metrics.dim_alpha)),
        );
    }

    div()
        .debug_selector({
            let id = id;
            move || format!("selection-card-{id}")
        })
        .absolute()
        .left(px(card.x))
        .top(px(card.y))
        .w(px(card.w))
        .h(px(card.h))
        .rounded_tl(px(metrics.radii[0]))
        .rounded_tr(px(metrics.radii[1]))
        .rounded_bl(px(metrics.radii[2]))
        .rounded_br(px(metrics.radii[3]))
        .child(content)
        .into_any_element()
}

/// 方勾单独一层，画在所有卡片之后，避免被相邻卡片盖住。
pub(crate) fn render_check(theme: Theme, id: ItemId, metrics: CardMetrics) -> AnyElement {
    let check = metrics.check;
    div()
        .debug_selector(move || format!("selection-check-{id}"))
        .absolute()
        .left(px(check.x))
        .top(px(check.y))
        .w(px(check.w))
        .h(px(check.h))
        .rounded(px(f32::from(selection::CHECK_RADIUS)))
        .bg(theme.accent)
        .opacity(metrics.check_opacity)
        .flex()
        .items_center()
        .justify_center()
        .child(icon::icon(Icon::Check, px(10.0), theme.accent_text))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::CardMetrics;
    use crate::ui::canvas::geometry::Rect;

    fn frame() -> Rect {
        Rect::new(10.0, 20.0, 100.0, 60.0)
    }

    #[test]
    fn selection_keeps_the_card_in_place_and_rounds_to_fourteen() {
        let metrics = CardMetrics::new(frame(), [0.0, 8.0, 12.0, 0.0], 1.0, 0.0, 1.0);
        // 选中不动位置：卡片必须和原图块完全重合，宽高和左上角都不差，
        // 否则内容会被拉伸，裁剪线和热区也会跟着错位。
        assert_eq!(metrics.card, frame());
        assert_eq!(metrics.radii, [14.0, 14.0, 14.0, 14.0]);
        assert_eq!(metrics.dim_alpha, 0.0);
    }

    #[test]
    fn card_stays_put_and_keeps_its_size_at_every_progress() {
        let frame = frame();
        for step in 0..=10 {
            let lift = step as f32 / 10.0;
            let metrics = CardMetrics::new(frame, [0.0; 4], lift, 0.0, 0.0);
            // 选中动画全程卡片都不该挪位置、也不该改尺寸：用户取消浮动就是
            // 因为上移会让卡片上边缘和裁剪热区错开，拖裁剪边时对不准。
            assert_eq!(metrics.card, frame, "进度 {lift} 时卡片应和图块完全重合");
        }
    }

    #[test]
    fn no_selection_keeps_original_frame_and_radii() {
        let metrics = CardMetrics::new(frame(), [0.0, 8.0, 12.0, 0.0], 0.0, 0.0, 0.0);
        assert_eq!(metrics.card, frame());
        assert_eq!(metrics.radii, [0.0, 8.0, 12.0, 0.0]);
        assert_eq!(metrics.check_opacity, 0.0);
    }

    #[test]
    fn check_sits_inside_lifted_card_top_right() {
        let metrics = CardMetrics::new(frame(), [0.0; 4], 1.0, 0.0, 1.0);
        let check = metrics.check;
        assert!((check.right() - (metrics.card.right() - 8.0)).abs() < 1e-6);
        assert!((check.y - (metrics.card.y + 8.0)).abs() < 1e-6);
        assert!((check.w - 18.0).abs() < 1e-6);
        assert_eq!(metrics.check_opacity, 1.0);
    }

    #[test]
    fn dim_alpha_matches_final_forty_five_percent() {
        let metrics = CardMetrics::new(frame(), [0.0; 4], 0.0, 1.0, 0.0);
        assert!((metrics.dim_alpha - 0.45).abs() < 1e-6);
    }
}