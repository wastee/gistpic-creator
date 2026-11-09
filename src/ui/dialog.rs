//! “这张图没找到字幕”的弹窗。

use gpui::prelude::*;
use gpui::{
    div, img, px, ClickEvent, Context, IntoElement, ObjectFit, StyledImage,
};

use crate::app::AppRoot;
use crate::imaging::crop::CropBand;
use crate::model::ItemId;
use crate::ui::theme::Theme;
use crate::ui::widgets;

impl AppRoot {
    pub(crate) fn render_dialog(
        &mut self,
        item_id: ItemId,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let found = self
            .project
            .items
            .iter()
            .find(|item| item.id == item_id)
            .map(|item| {
                (
                    item.id,
                    item.rgb.clone(),
                    item.width,
                    item.height,
                    item.name.clone(),
                )
            });

        let (name, size_text, thumbnail) = match found {
            Some((id, rgb, width, height, name)) => {
                let image = self.preview_for(
                    id,
                    rgb.as_ref(),
                    CropBand {
                        y0: 0,
                        y1: height,
                    },
                    520,
                );
                (name, format!("{width}×{height}"), Some(image))
            }
            None => (String::new(), String::new(), None),
        };

        let mut card = div()
            .w(px(540.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .rounded(px(12.))
            .bg(theme.panel)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_lg()
                    .text_color(theme.text)
                    .child("这张图没找到字幕"),
            )
            .child(div().text_sm().text_color(theme.muted).child(
                "可以保留整张图，或者跳过它。跳过会把它从列表里移除，之后可以撤销。",
            ));

        if let Some(image) = thumbnail {
            card = card.child(
                div().flex().justify_center().child(
                    img(image)
                        .w_full()
                        .h(px(230.))
                        .rounded(px(6.))
                        .object_fit(ObjectFit::Contain),
                ),
            );
        }

        card = card
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .child(format!("{name} · {size_text}")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(10.))
                    .child(widgets::tool_button(
                        "dialog-skip",
                        "跳过这张（移除）",
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.confirm_dialog(false, cx);
                        }),
                    ))
                    .child(widgets::primary_button(
                        "dialog-keep",
                        "整张图保留",
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.confirm_dialog(true, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .child("按 Esc 跳过"),
            );

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.overlay_color())
            .child(card)
    }
}
