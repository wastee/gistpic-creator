//! 作品左侧的缝隙与颜色控件。
//!
//! 字幕模式和排版模式共用同一个控件：上半是缝隙图标，下半是颜色圆点。鼠标压上去
//! 作品的每条接缝一起亮起；上下拖动改缝隙宽度，点圆点换缝隙颜色。接缝自己不再接
//! 鼠标，免得和图片边缘的裁剪热区抢同一块位置。

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, DragMoveEvent, MouseButton, MouseDownEvent,
    MouseUpEvent, div, px,
};

use crate::app::{AppRoot, GapDrag};
use crate::ui::canvas::{CanvasLayout, Rect};
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, GAP_CONTROL_DOT, radius, text_size};
use crate::ui::widgets;

/// 像素气泡左边缘与药丸右边缘之间的距离。
const BUBBLE_OFFSET: f32 = 4.0;

/// 像素气泡的矩形，坐标原点在作品左上角。
///
/// 气泡挂在药丸右边，顶边与药丸顶边对齐。单独写成纯函数，摆放位置可以单独测。
pub fn gap_bubble_rect(control: Rect) -> Rect {
    Rect::new(control.right() + BUBBLE_OFFSET, control.y, 0.0, 0.0)
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

        let mut control = div()
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

        // 拖动中才显示像素气泡，贴在药丸右边，顶边对齐。
        if self.gap_drag.is_some() {
            let bubble = gap_bubble_rect(rect);
            control = control.child(
                div()
                    .debug_selector(|| "gap-bubble".to_string())
                    .absolute()
                    .left(px(bubble.x - rect.x))
                    .top(px(bubble.y - rect.y))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(radius::PILL)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border_subtle)
                    .shadow(theme.shadow_bubble())
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.muted)
                    .child(
                        self.language
                            .gap_px_label(self.project.settings.gap.effective_px()),
                    ),
            );
        }
        Some(control.into_any_element())
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

    #[test]
    fn bubble_sits_right_of_the_pill_and_aligns_with_its_top() {
        let control = Rect::new(-36.0, 100.0, 28.0, 56.0);
        let bubble = gap_bubble_rect(control);
        assert!(
            bubble.x > control.right(),
            "气泡要在药丸右边：气泡 {bubble:?} 药丸 {control:?}"
        );
        assert!(
            (bubble.y - control.y).abs() < 1e-6,
            "气泡顶边与药丸顶边对齐"
        );
    }
}
