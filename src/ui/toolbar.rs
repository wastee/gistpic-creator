//! 顶部工具栏：模式切换、导入、撤销、导出、缩放。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, IntoElement, SharedString, div, px};

use crate::app::AppRoot;
use crate::app::actions::*;
use crate::model::Mode;

use super::theme::Theme;
use super::widgets;

impl AppRoot {
    pub(crate) fn render_toolbar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.project.mode;
        let has_images = !self.project.items.is_empty();
        let can_undo = self.undo.can_undo();
        let can_redo = self.undo.can_redo();

        let mode_switch = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(theme.panel_alt)
            .child(widgets::chip(
                SharedString::from("mode-subtitle"),
                self.language.mode_label(Mode::Subtitle),
                mode == Mode::Subtitle,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(Mode::Subtitle, cx)
                }),
            ))
            .child(widgets::chip(
                SharedString::from("mode-layout"),
                self.language.mode_label(Mode::Layout),
                mode == Mode::Layout,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(Mode::Layout, cx)
                }),
            ));

        let zoom = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .child(self.language.zoom()),
            )
            .child(widgets::icon_button(
                "zoom-out",
                "−",
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.action_zoom_out(&ZoomOut, window, cx)
                }),
            ))
            .child(div().w(px(150.)).child(widgets::slider(
                "zoom",
                self.zoom_ratio(),
                21,
                theme,
                cx,
                Self::set_zoom_index,
            )))
            .child(widgets::icon_button(
                "zoom-in",
                "+",
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.action_zoom_in(&ZoomIn, window, cx)
                }),
            ))
            .child(
                div()
                    .w(px(42.))
                    .flex()
                    .justify_end()
                    .text_xs()
                    .text_color(theme.muted)
                    .child(format!("{:.0}%", self.zoom * 100.0)),
            );

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .h(px(54.))
            .px(px(14.))
            .bg(theme.panel)
            .border_color(theme.border)
            .border_b_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text)
                            .child(self.language.app_name()),
                    )
                    .child(mode_switch)
                    .child(widgets::divider_vertical(theme))
                    .child(widgets::primary_button(
                        "paste-image",
                        self.language
                            .paste_image(&crate::platform::shortcut_label("V")),
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_paste(&PasteImage, window, cx)
                        }),
                    ))
                    .child(widgets::tool_button(
                        "open-images",
                        self.language.open_images(),
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_open(&OpenImages, window, cx)
                        }),
                    ))
                    .child(widgets::tool_button(
                        "clear-all",
                        self.language.clear_all(),
                        theme,
                        has_images,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_clear(&ClearAll, window, cx)
                        }),
                    ))
                    .child(widgets::divider_vertical(theme))
                    .child(widgets::tool_button(
                        "undo",
                        self.language.undo(),
                        theme,
                        can_undo,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_undo(&UndoEdit, window, cx)
                        }),
                    ))
                    .child(widgets::tool_button(
                        "redo",
                        self.language.redo(),
                        theme,
                        can_redo,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_redo(&RedoEdit, window, cx)
                        }),
                    ))
                    .child(widgets::divider_vertical(theme))
                    .child(widgets::primary_button(
                        "export-png",
                        self.language.export_png(),
                        theme,
                        has_images,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_export(&ExportPng, window, cx)
                        }),
                    ))
                    .child(widgets::tool_button(
                        "copy-image",
                        self.language.copy_image(),
                        theme,
                        has_images,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_copy(&CopyImage, window, cx)
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(zoom)
                    .child(widgets::divider_vertical(theme))
                    .child(widgets::tool_button(
                        "quit-app",
                        self.language.quit(),
                        theme,
                        true,
                        cx.listener(|_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            cx.quit();
                        }),
                    ))
                    .child(widgets::icon_button(
                        "quit",
                        "×",
                        theme,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.on_close_requested(window, cx);
                        }),
                    )),
            )
    }
}
