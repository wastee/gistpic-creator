//! 画布右上角的关闭按钮。
//!
//! 画布保持无顶栏，系统窗口边框继续负责最小化和最大化；
//! 这里只补一个轻量的关闭入口，避免用户找不到窗口关闭动作。
//!
//! 关闭按钮默认很淡，降低视觉存在感；指针进入或靠近窗口右上角区域时，
//! 才淡入并变亮，离开后再淡回安静状态。透明度按时间插值，不包动画层，
//! 避免动画 id 改变按钮的元素状态路径、把快速点击吞掉。

use gpui::prelude::*;
use gpui::{Context, CursorStyle, IntoElement, Point, Size, WindowControlArea, div, px};

use crate::app::AppRoot;
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space};

/// 指针是否落在窗口右上角的“关闭按钮显示区”里。
pub(crate) fn close_region_revealed(
    pointer: Point<gpui::Pixels>,
    viewport: Size<gpui::Pixels>,
) -> bool {
    let right = f32::from(viewport.width);
    let x = f32::from(pointer.x);
    let y = f32::from(pointer.y);
    y >= 0.0 && y <= tokens::CLOSE_REVEAL_H && x >= right - tokens::CLOSE_REVEAL_W
}

/// 点击是否正好落在右上角关闭按钮本身。
///
/// 菜单和倒计时这两层铺满整个窗口，会先吃到关闭按钮的点击。
/// 这两层需要认出落在关闭按钮上的点击并照常关闭窗口，
/// 不能只把浮层关掉、让窗口留在屏幕上。
pub(crate) fn close_button_hit(pointer: Point<gpui::Pixels>, viewport: Size<gpui::Pixels>) -> bool {
    let right = f32::from(viewport.width);
    let x = f32::from(pointer.x);
    let y = f32::from(pointer.y);
    let inset = f32::from(space::MD);
    let side = f32::from(size::ICON_BUTTON);
    x >= right - inset - side && x <= right - inset && y >= inset && y <= inset + side
}

impl AppRoot {
    /// 右上角的单个关闭按钮，不扩展成一排常驻窗口按钮。
    pub(crate) fn render_window_controls(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover_fade = self
            .hover_gate
            .fade_delta(&HoverKey::widget("window-close"));
        let revealed = self.close_near;
        let opacity = self.close_button_opacity();
        let icon_color = if revealed { theme.text } else { theme.faint };
        let base = if revealed {
            Some(theme.surface_elevated)
        } else {
            None
        };
        let close = div()
            .debug_selector(|| "window-close".to_string())
            .id("window-close")
            .flex()
            .items_center()
            .justify_center()
            .w(size::ICON_BUTTON)
            .h(size::ICON_BUTTON)
            .rounded(radius::SMALL)
            .bg(hover::fade_background(base, theme.danger, hover_fade))
            .border_1()
            .border_color(if revealed {
                theme.border_subtle
            } else {
                theme.border_subtle.opacity(0.0)
            })
            .cursor(CursorStyle::PointingHand)
            .window_control_area(WindowControlArea::Close)
            .on_hover(self.widget_hover_listener("window-close", cx))
            // 不依赖 click：Wayland 下指针贴着窗口右上角时，按下后合成器会立刻发
            // Leave 清掉焦点窗口，随后的松开事件被平台丢弃，click 需要的一对
            // 按下/松开永远凑不齐，按钮就没反应。左键按下就直接执行关闭，
            // 松开事件丢不丢都不影响，绕过这个平台缺陷。
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseDownEvent, window, cx| {
                    this.on_close_requested(window, cx);
                }),
            )
            .child(icon::icon(Icon::Close, px(14.), icon_color))
            .opacity(opacity);
        div()
            .debug_selector(|| "window-controls".to_string())
            .id("window-controls")
            .absolute()
            .top(space::MD)
            .right(space::MD)
            .child(close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    fn viewport() -> Size<gpui::Pixels> {
        Size {
            width: px(1280.),
            height: px(860.),
        }
    }

    #[test]
    fn close_button_reveals_only_near_top_right() {
        let viewport = viewport();
        // 右上角区域里
        assert!(close_region_revealed(
            gpui::point(px(1260.), px(40.)),
            viewport
        ));
        // 正中间：不显示
        assert!(!close_region_revealed(
            gpui::point(px(640.), px(430.)),
            viewport
        ));
        // 左边偏上：不在右上角区域
        assert!(!close_region_revealed(
            gpui::point(px(400.), px(40.)),
            viewport
        ));
        // 右边但太靠下：不显示
        assert!(!close_region_revealed(
            gpui::point(px(1260.), px(400.)),
            viewport
        ));
    }
}
