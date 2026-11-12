//! 作品左侧的缝隙与颜色控件。
//!
//! 字幕模式和排版模式共用同一个控件：上半是缝隙图标，下半是颜色圆点。鼠标压上去
//! 作品的每条接缝一起亮起；上下拖动改缝隙宽度，点圆点换缝隙颜色。接缝自己不再接
//! 鼠标，免得和图片边缘的裁剪热区抢同一块位置。

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, CursorStyle, DragMoveEvent,
    MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, Size, Window, div, ease_out_quint, px,
};

use crate::app::{AppRoot, GapDrag};
use crate::ui::canvas::{CanvasLayout, Rect};
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, GAP_CONTROL_DOT, GAP_MAX, radius, space, text_size};
use crate::ui::widgets;

/// 气泡尺寸：横排，留得下 "100 px" 与下面那条 accent 刻度。
pub const GAP_BUBBLE_W: f32 = 64.0;
pub const GAP_BUBBLE_H: f32 = 24.0;
/// 气泡与指针的间距 / 下移量，以及气泡内刻度的长度。全为 4 的倍数。
const BUBBLE_OFFSET: f32 = 12.0;
const BUBBLE_DROP: f32 = 16.0;
const BUBBLE_TRACK_W: f32 = 40.0;

/// 气泡在窗口里的位置：跟手、横排、不遮挡作品、留在视口内。
///
/// `pointer`、`artwork_bounds` 都是窗口坐标。独立成纯函数，摆放规则可单测。
/// 首选指针右下方；压到作品就翻到指针左侧（缝隙控件本来就在作品左边）。
/// 夹进视口后若仍压到作品（窄窗口左侧放不下），再退到作品上方或下方。
pub fn gap_bubble_rect(
    pointer: Point<Pixels>,
    artwork_bounds: Rect,
    viewport: Size<Pixels>,
) -> Rect {
    let px = f32::from(pointer.x);
    let py = f32::from(pointer.y);
    let max_x = (f32::from(viewport.width) - GAP_BUBBLE_W).max(0.0);
    let max_y = (f32::from(viewport.height) - GAP_BUBBLE_H).max(0.0);

    let mut x = px + BUBBLE_OFFSET;
    if Rect::new(x, py + BUBBLE_DROP, GAP_BUBBLE_W, GAP_BUBBLE_H).intersects(&artwork_bounds) {
        x = px - BUBBLE_OFFSET - GAP_BUBBLE_W;
    }
    x = x.clamp(0.0, max_x);
    let mut y = (py + BUBBLE_DROP).clamp(0.0, max_y);
    let mut rect = Rect::new(x, y, GAP_BUBBLE_W, GAP_BUBBLE_H);

    if rect.intersects(&artwork_bounds) {
        y = (artwork_bounds.y - GAP_BUBBLE_H - BUBBLE_OFFSET).clamp(0.0, max_y);
        rect.y = y;
        if rect.intersects(&artwork_bounds) {
            rect.y = (artwork_bounds.bottom() + BUBBLE_OFFSET).clamp(0.0, max_y);
        }
    }
    rect
}

impl AppRoot {
    /// 作品左侧的缝隙与颜色控件。作品里没有接缝时返回 `None`，整个控件不画。
    pub(crate) fn render_gap_control(
        &mut self,
        theme: Theme,
        layout: &CanvasLayout,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rect = layout.gap_control_rect()?;
        let key = HoverKey::widget("gap-control");
        let fade = self.hover_gate.fade_delta(&key);
        let hovered = self.hover_gate.is_active(&key);
        let gap = self.project.settings.gap;
        let icon_color = if hovered { theme.text } else { theme.muted };
        let dot = f32::from(GAP_CONTROL_DOT);

        // 上半：缝隙图标。悬停时从灰变亮。
        let icon_half = div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(icon::icon(Icon::Gap, px(14.), icon_color));
        // 下半：颜色圆点，自带一圈描边，白点落在浅底上也要看得见。
        let dot_half = div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .debug_selector(|| "gap-control-dot".to_string())
                    .id("gap-control-dot")
                    .w(px(dot))
                    .h(px(dot))
                    .rounded(px(dot / 2.0))
                    .bg(widgets::quad_color(gap.color))
                    .border_1()
                    .border_color(theme.border_subtle)
                    .cursor(CursorStyle::PointingHand)
                    // 点圆点只换颜色：按下就掐断冒泡，药丸收不到，不会带出宽度拖动。
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .on_click(
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.cycle_gap_color(cx);
                        }),
                    ),
            );

        let control = div()
            .debug_selector(|| "gap-control".to_string())
            .id("gap-control")
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .flex()
            .flex_col()
            .items_center()
            .font_family(tokens::FONT_FAMILY)
            .rounded(radius::PILL)
            .bg(hover::fade_background(
                Some(theme.surface),
                theme.surface_elevated,
                fade,
            ))
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .cursor(CursorStyle::ResizeUpDown)
            .on_hover(
                cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("gap-control"), *hovered, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, event: &MouseDownEvent, _window, cx| {
                    this.begin_gap_drag(event, cx);
                    // 控件站在画布留白里，不掐断冒泡就会被当成“空白处拖窗口”。
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_drag(GapDrag, |_value, _offset, _window, cx| {
                cx.new(|_| GapDragGhost)
            })
            .on_drag_move::<GapDrag>(cx.listener(
                |this: &mut AppRoot, event: &DragMoveEvent<GapDrag>, _window, cx| {
                    this.drag_gap(event, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &MouseUpEvent, _window, cx| {
                    this.end_gap_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &MouseUpEvent, _window, cx| {
                    this.end_gap_drag(cx);
                }),
            )
            .child(icon_half)
            .child(dot_half);

        Some(control.into_any_element())
    }

    /// 拖动缝隙时的横排像素气泡，画在根节点（窗口级浮层），不受画布
    /// `.overflow_scroll()` 裁切，也不会被作品盖住或盖住作品。
    pub(crate) fn render_gap_bubble(
        &mut self,
        theme: Theme,
        window: &Window,
    ) -> Option<AnyElement> {
        let pointer = self.gap_drag.as_ref()?.pointer;

        let viewport = window.viewport_size();
        let layout = self.canvas_layout();
        let origin = layout.origin(viewport, self.canvas_scroll.offset());
        let artwork = Rect::new(origin.x, origin.y, layout.width, layout.height);
        let rect = gap_bubble_rect(pointer, artwork, viewport);

        let gap_px = self.project.settings.gap.effective_px();
        let label = self.language.gap_px_label(gap_px);
        // 间隙程度：气泡内一条极细 accent 刻度，不引入独立进度条 widget。
        let fill_w = (BUBBLE_TRACK_W * gap_px as f32 / GAP_MAX as f32).clamp(0.0, BUBBLE_TRACK_W);

        let bubble = div()
            .debug_selector(|| "gap-bubble".to_string())
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .px(space::SM)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .rounded(radius::PILL)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(theme.muted)
            // GPUI 默认 WhiteSpace::Normal 会在 "100 px" 的空格处折行，
            // 气泡变竖；强制不换行才是横排。
            .whitespace_nowrap()
            .child(label)
            .child(
                div()
                    .debug_selector(|| "gap-bubble-scale".to_string())
                    .w(px(BUBBLE_TRACK_W))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(theme.border_subtle)
                    .child(div().w(px(fill_w)).h(px(2.)).rounded(px(1.)).bg(theme.accent)),
            );

        Some(
            bubble
                .with_animation(
                    "gap-bubble-enter",
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element(),
        )
    }
}

/// 拖缝隙控件时要交给 GPUI 的那个跟手元素。
///
/// 拖动只需要 `GapDrag` 这个类型标识，不需要真的画一张跟手小图，所以画个空盒子。
pub(crate) struct GapDragGhost;

impl gpui::Render for GapDragGhost {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn artwork() -> Rect {
        Rect::new(200.0, 100.0, 800.0, 600.0)
    }

    fn viewport() -> Size<Pixels> {
        size(px(1920.), px(1080.))
    }

    #[test]
    fn bubble_is_horizontal_and_never_covers_artwork() {
        // 指针在作品左侧的缝隙控件上，首选位置会伸进作品，应该翻到左侧。
        let bubble = gap_bubble_rect(point(px(178.), px(300.)), artwork(), viewport());
        assert_eq!(bubble.w, GAP_BUBBLE_W);
        assert_eq!(bubble.h, GAP_BUBBLE_H);
        assert!(bubble.w > bubble.h, "气泡应是横排的");
        assert!(
            !bubble.intersects(&artwork()),
            "气泡不应遮挡作品：{bubble:?}"
        );
        assert!(bubble.x >= 0.0 && bubble.right() <= 1920.0, "横向留在视口内");
        assert!(bubble.y >= 0.0 && bubble.bottom() <= 1080.0, "纵向留在视口内");
    }

    #[test]
    fn bubble_stays_off_artwork_even_in_a_narrow_window() {
        let narrow_artwork = Rect::new(48.0, 100.0, 260.0, 400.0);
        let viewport = size(px(320.), px(600.));
        let bubble = gap_bubble_rect(point(px(40.), px(300.)), narrow_artwork, viewport);
        assert!(
            !bubble.intersects(&narrow_artwork),
            "窄窗口左侧放不下时也要避开作品：{bubble:?}"
        );
        assert!(bubble.x >= 0.0 && bubble.right() <= 320.0);
        assert!(bubble.y >= 0.0 && bubble.bottom() <= 600.0);
    }

    #[test]
    fn bubble_follows_the_pointer_vertically() {
        let a = gap_bubble_rect(point(px(178.), px(300.)), artwork(), viewport());
        let b = gap_bubble_rect(point(px(178.), px(420.)), artwork(), viewport());
        assert!(
            (b.y - a.y - 120.0).abs() < 1e-6,
            "气泡纵向应 1:1 跟随指针：{a:?} -> {b:?}"
        );
    }
}
