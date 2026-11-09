//! 底部状态栏：消息、参与输出数量、输出尺寸、缩放。

use gpui::prelude::*;
use gpui::{div, px, Context, IntoElement};

use crate::app::AppRoot;
use crate::imaging::crop::{self, Inclusion};
use crate::ui::theme::Theme;

impl AppRoot {
    pub(crate) fn render_status_bar(
        &mut self,
        theme: Theme,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let total = self.project.items.len();
        let inclusions = crop::resolve(&self.project);
        let active = inclusions
            .iter()
            .filter(|inclusion| !matches!(inclusion, Inclusion::Skipped))
            .count();

        let output_text = self
            .output_size()
            .map(|(width, height)| format!("输出 {width}×{height}"))
            .unwrap_or_else(|| "输出 —".to_string());

        let busy = if self.decoding > 0 {
            "正在读取图片…"
        } else if self.detecting {
            "正在识别字幕…"
        } else {
            ""
        };

        let message_color = if self.status.error {
            theme.danger
        } else {
            theme.muted
        };

        div()
            .h(px(30.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .px(px(14.))
            .bg(theme.panel_alt)
            .border_color(theme.border)
            .border_t_1()
            .text_xs()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_color(message_color)
                            .child(self.status.text.clone()),
                    )
                    .child(
                        div()
                            .text_color(theme.accent)
                            .child(busy),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .text_color(theme.muted)
                    .child(div().child(format!("参与输出 {active}/{total} 张")))
                    .child(div().child(output_text)),
            )
    }
}
