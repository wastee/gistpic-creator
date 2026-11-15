//! 作品左侧的缝隙与颜色控件。
//!
//! 字幕模式和排版模式共用同一个控件：上半是缝隙图标，下半是颜色圆点。鼠标压上去
//! 作品的每条接缝一起亮起；上下拖动改缝隙宽度，点圆点换缝隙颜色。接缝自己不再接
//! 鼠标，免得和图片边缘的裁剪热区抢同一块位置。

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, DragMoveEvent, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Size, Window, div, px,
};

use crate::app::{AppRoot, GapDrag};
use crate::ui::canvas::Rect;
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, GAP_CONTROL_DOT, GAP_MAX, radius, text_size};
use crate::ui::widgets;

/// 需求 13：间隔数值提示的占位宽度与高度。
///
/// 提示本身不画背景、不画边框，只有一行文字加一条很淡的刻度；这块矩形只是
/// 用来把它摆在胶囊正下方并居中，宽一点才能在胶囊窄的时候也放得下字。
pub const GAP_LABEL_W: f32 = 120.0;
pub const GAP_LABEL_H: f32 = 18.0;
/// 提示与胶囊底边的间距，以及提示里那条刻度的长度。
const LABEL_GAP: f32 = 10.0;
const LABEL_TRACK_W: f32 = 40.0;

/// 需求 13：间隔数值提示的矩形，固定在胶囊正下方并水平居中。
///
/// `capsule` 是间隔胶囊在窗口里的矩形。提示挂在它下面，不跟随鼠标：拖动时
/// 数值就在眼皮底下换，视线不用在两个位置之间来回找。
///
/// 横向夹进视口：胶囊贴近窗口左右边缘时，提示不会有一半跑到窗口外面。
pub fn gap_label_rect(capsule: Rect, viewport: Size<Pixels>) -> Rect {
    let viewport_w = f32::from(viewport.width);
    let viewport_h = f32::from(viewport.height);
    let centered = capsule.x + (capsule.w - GAP_LABEL_W) / 2.0;
    let max_x = (viewport_w - GAP_LABEL_W).max(0.0);
    let mut y = capsule.bottom() + LABEL_GAP;
    // 胶囊贴近窗口底部时，提示改摆在它上面，免得被窗口裁掉。
    if y + GAP_LABEL_H > viewport_h {
        y = (capsule.y - LABEL_GAP - GAP_LABEL_H).max(0.0);
    }
    Rect::new(centered.clamp(0.0, max_x), y, GAP_LABEL_W, GAP_LABEL_H)
}

impl AppRoot {
    /// 作品左侧的缝隙与颜色控件。作品里没有接缝时返回 `None`，整个控件不画。
    pub(crate) fn render_gap_control(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        let layout = self.canvas_layout();
        let rect = layout.gap_control_rect(viewport, self.canvas_scroll.offset())?;
        let key = HoverKey::widget("gap-control");
        let fade = self.hover_gate.fade_delta(&key);
        let hovered = self.hover_gate.is_active(&key);
        let gap = self.project.settings.gap;
        let icon_color = if hovered { theme.text } else { theme.muted };
        let dot = f32::from(GAP_CONTROL_DOT);

        // 需求 11：只有这上半块「拖动按钮」能改间隔，胶囊外沿、图标以外的
        // 空白、以及下半的颜色圆点都不启动拖动。
        //
        // 早先整个药丸都挂拖动：点在圆角外沿上也会改间隔，用户只是想点圆点
        // 换颜色却把缝隙拖动了。现在拖动只挂在这一块上，药丸本体只负责外观
        // 和悬停上报。
        let handle_side = f32::from(tokens::GAP_CONTROL_HANDLE);
        let handle = div()
            .debug_selector(|| "gap-control-handle".to_string())
            .id("gap-control-handle")
            .w(px(handle_side))
            .h(px(handle_side))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::PILL)
            .cursor(CursorStyle::ResizeUpDown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, event: &MouseDownEvent, _window, cx| {
                    this.begin_gap_drag(event, cx);
                    // 控件站在画布留白里，不掐断冒泡就会被当成“空白处拖窗口”。
                    cx.stop_propagation();
                }),
            )
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
            .child(icon::icon(Icon::Gap, px(14.), icon_color));
        let icon_half = div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(handle);
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

        // 药丸本体：只负责外观、悬停上报和掐断右键冒泡，不接任何拖动。
        // 需求 11：点在外沿上不该改间隔。
        //
        // 需求 12：默认半透明，指针靠近才不透明，离开后按时间淡回去。
        let opacity = self.gap_control_opacity_at(cx.background_executor().now());
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
            .opacity(opacity)
            .on_hover(
                cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("gap-control"), *hovered, cx);
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(icon_half)
            .child(dot_half);

        Some(control.into_any_element())
    }

    /// 需求 13：间隔数值提示，固定显示在间隔胶囊正下方。
    ///
    /// 只在拖动间隔时出现，松手后淡出；位置固定在胶囊下方，不跟随鼠标。
    /// 不画背景也不画边框，就是一行轻量文字加一条很淡的刻度，不抢画面注意力。
    ///
    /// 画在根节点（窗口级浮层），不受画布 `.overflow_scroll()` 裁切。
    pub(crate) fn render_gap_bubble(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let opacity = self.gap_label_fade.value_at(cx.background_executor().now());
        // 没在拖、也已经淡完：整块不画。
        if self.gap_drag.is_none() && opacity <= 0.001 {
            return None;
        }

        let viewport = window.viewport_size();
        let layout = self.canvas_layout();
        let capsule = layout.gap_control_rect(viewport, self.canvas_scroll.offset())?;
        let rect = gap_label_rect(capsule, viewport);

        let gap_px = self.project.settings.gap.effective_px();
        let label = self.language.gap_px_label(gap_px);
        // 间隙程度：一条极细 accent 刻度，不引入独立进度条 widget。
        let fill_w = (LABEL_TRACK_W * gap_px as f32 / GAP_MAX as f32).clamp(0.0, LABEL_TRACK_W);

        let bubble = div()
            .debug_selector(|| "gap-bubble".to_string())
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(theme.muted)
            .opacity(opacity)
            // GPUI 默认 WhiteSpace::Normal 会在 "100 px" 的空格处折行，
            // 提示变竖；强制不换行才是横排。
            .whitespace_nowrap()
            .child(label)
            .child(
                div()
                    .debug_selector(|| "gap-bubble-scale".to_string())
                    .w(px(LABEL_TRACK_W))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(theme.border_subtle)
                    .child(div().w(px(fill_w)).h(px(2.)).rounded(px(1.)).bg(theme.accent)),
            );

        Some(bubble.into_any_element())
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
    use gpui::size;

    /// 一个摆在作品左侧的胶囊，纵向在视口中间。
    fn capsule() -> Rect {
        Rect::new(160.0, 500.0, 28.0, 56.0)
    }

    fn viewport() -> Size<Pixels> {
        size(px(1920.), px(1080.))
    }

    /// 需求 13：提示固定在胶囊正下方、水平居中。
    #[test]
    fn the_gap_label_sits_below_the_capsule_and_centred() {
        let label = gap_label_rect(capsule(), viewport());
        let capsule = capsule();
        assert!(
            label.y >= capsule.bottom(),
            "提示应在胶囊下方：胶囊底 {:.1}，提示顶 {:.1}",
            capsule.bottom(),
            label.y
        );
        let capsule_mid = capsule.x + capsule.w / 2.0;
        let label_mid = label.x + label.w / 2.0;
        assert!(
            (label_mid - capsule_mid).abs() < 1e-6,
            "提示应在胶囊正下方居中：胶囊中心 {capsule_mid:.1}，提示中心 {label_mid:.1}"
        );
    }

    /// 需求 13：提示不跟随鼠标，胶囊不动它就不动。
    #[test]
    fn the_gap_label_does_not_follow_the_pointer() {
        let first = gap_label_rect(capsule(), viewport());
        let second = gap_label_rect(capsule(), viewport());
        assert_eq!(first.x, second.x);
        assert_eq!(first.y, second.y);
    }

    /// 胶囊贴近窗口左右边缘时，提示夹在视口里，不会露到窗口外面。
    #[test]
    fn the_gap_label_stays_inside_a_narrow_window() {
        let narrow = size(px(120.), px(600.));
        for x in [0.0_f32, 100.0] {
            let capsule = Rect::new(x, 200.0, 28.0, 56.0);
            let label = gap_label_rect(capsule, narrow);
            assert!(
                label.x >= 0.0 && label.right() <= 120.0,
                "提示应留在视口内：{label:?}"
            );
        }
    }

    /// 胶囊贴近窗口底部、下面放不下时，提示改摆到它上面，不会被窗口裁掉。
    #[test]
    fn the_gap_label_flips_above_a_low_capsule() {
        // 胶囊底边离窗口底只剩 4 像素，下方放不下 18 像素高的提示。
        let low = Rect::new(400.0, 1020.0, 28.0, 56.0);
        let label = gap_label_rect(low, viewport());
        assert!(
            label.bottom() <= 1080.0,
            "提示不该被窗口底边裁掉：{label:?}"
        );
        assert!(
            label.bottom() <= low.y,
            "放不下时应翻到胶囊上方：胶囊顶 {:.1}，提示底 {:.1}",
            low.y,
            label.bottom()
        );
    }
}
