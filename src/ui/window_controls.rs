//! 画布右上角的关闭按钮。
//!
//! 画布保持无顶栏，系统窗口边框继续负责最小化和最大化；
//! 这里只补一个轻量的关闭入口，避免用户找不到窗口关闭动作。

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, CursorStyle, IntoElement, WindowControlArea, div, px,
};
use std::time::Instant;

use crate::app::AppRoot;
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{radius, size, space};

impl AppRoot {
    /// 右上角的单个关闭按钮，不扩展成一排常驻窗口按钮。
    /// 悬停变红要走全局延迟，避免扫过右上角时误闪红色。
    pub(crate) fn render_window_controls(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fade = self
            .hover_gate
            .fade_delta(&HoverKey::widget("window-close"), Instant::now());
        let close = div()
            .debug_selector(|| "window-close".to_string())
            .id("window-close")
            .flex()
            .items_center()
            .justify_center()
            .w(size::ICON_BUTTON)
            .h(size::ICON_BUTTON)
            .rounded(radius::SMALL)
            .bg(hover::fade_background(Some(theme.surface), theme.danger, fade))
            .border_1()
            .border_color(theme.border_subtle)
            .cursor(CursorStyle::PointingHand)
            .window_control_area(WindowControlArea::Close)
            .on_hover(self.widget_hover_listener("window-close", cx))
            .on_click(
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.on_close_requested(window, cx);
                }),
            )
            .child(icon::icon(Icon::Close, px(14.), theme.muted));
        div()
            .debug_selector(|| "window-controls".to_string())
            .id("window-controls")
            .absolute()
            .top(space::MD)
            .right(space::MD)
            .child(close)
    }
}
