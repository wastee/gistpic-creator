//! 底部状态栏：一条消息和输出尺寸。

use gpui::prelude::*;
use gpui::{Context, IntoElement, div, px};

use crate::app::AppRoot;
use crate::imaging::crop::{self, Inclusion};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, size, space, text_size};

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
            .map(|(width, height)| self.language.output_size(width, height))
            .unwrap_or_else(|| self.language.output_none().to_string());

        let busy = if self.decoding > 0 {
            self.language.reading_images()
        } else if self.detecting {
            self.language.detecting_subtitles()
        } else {
            ""
        };

        let message_color = if self.status.error {
            theme.danger
        } else {
            theme.muted
        };

        div()
            .debug_selector(|| "status-bar".to_string())
            .h(size::STATUS_BAR)
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .gap(space::MD)
            .px(space::MD)
            .bg(theme.bg)
            .text_size(text_size::CAPTION)
            .font_family(tokens::FONT_FAMILY)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::SM)
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_color(message_color)
                            .child(self.status.text.clone()),
                    )
                    .child(div().text_color(theme.muted).child(busy)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::MD)
                    .text_color(theme.faint)
                    .child(div().child(self.language.active_output(active, total)))
                    .child(div().child(output_text)),
            )
    }
}
