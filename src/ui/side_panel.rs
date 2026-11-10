//! 右侧设置面板。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, IntoElement, SharedString, div, px};

use crate::app::AppRoot;
use crate::i18n::Language;
use crate::model::{ItemStatus, LayoutTarget, Mode, Rgba};

use super::theme::Theme;
use super::widgets;

impl AppRoot {
    pub(crate) fn render_side_panel(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut content = div().flex().flex_col().gap(px(14.)).w_full();

        match self.project.mode {
            Mode::Subtitle => {
                content = content.child(self.subtitle_settings(theme, cx));
            }
            Mode::Layout => {
                content = content.child(self.layout_settings(theme, cx));
            }
        }

        content = content
            .child(widgets::divider(theme))
            .child(self.gap_settings(theme, cx))
            .child(widgets::divider(theme))
            .child(self.window_settings(theme, cx))
            .child(widgets::divider(theme))
            .child(self.language_settings(theme, cx))
            .child(widgets::divider(theme))
            .child(self.selection_settings(theme, cx));

        div()
            .w(px(312.))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .border_color(theme.border)
            .border_l_1()
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p(px(14.))
                    .child(content),
            )
    }

    fn subtitle_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &self.project.settings;
        let detected = self
            .project
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Ready)
            .count();
        let missing = self
            .project
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::NoSubtitle)
            .count();

        let bleed_percent = (settings.bleed * 100.0).round() as u32;
        let bleed_ratio = (settings.bleed / 0.30).clamp(0.0, 1.0);

        let mut section = div().flex().flex_col().gap(px(10.)).w_full();
        section = section
            .child(widgets::switch_row(
                "toggle-crop",
                self.language.crop_subtitles(),
                settings.crop,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_crop(!this.project.settings.crop, cx);
                }),
            ))
            .child(widgets::switch_row(
                "toggle-keep-second",
                self.language.keep_second_line(),
                settings.keep_second,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_keep_second(!this.project.settings.keep_second, cx);
                }),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .text_color(theme.text)
                            .child(self.language.bleed())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted)
                                    .child(format!("{bleed_percent}%")),
                            ),
                    )
                    .child(widgets::slider(
                        "bleed",
                        bleed_ratio,
                        7,
                        theme,
                        cx,
                        Self::set_bleed_index,
                    )),
            )
            .child(widgets::hint_text(self.language.bleed_hint(), theme));

        if !self.project.items.is_empty() {
            section = section.child(widgets::hint_text(
                self.language.detected_summary(detected, missing),
                theme,
            ));
        }

        if settings.crop {
            section = section.child(widgets::hint_text(
                self.language.subtitle_crop_hint(),
                theme,
            ));
        }

        section
    }

    fn layout_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.project.settings.layout;
        let row = |presets: &[LayoutTarget]| {
            let mut row = div().flex().items_center().gap(px(6.)).w_full();
            for target in presets {
                let target = *target;
                row = row.child(widgets::chip(
                    SharedString::from(format!("layout-{}", target.key())),
                    self.language.layout_label(target),
                    current == target,
                    theme,
                    cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                        this.set_layout_target(target, cx);
                    }),
                ));
            }
            row
        };

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(widgets::section_title(self.language.arrangement(), theme))
            .child(row(&[
                LayoutTarget::Aspect(16.0 / 9.0),
                LayoutTarget::Aspect(9.0 / 16.0),
            ]))
            .child(row(&[
                LayoutTarget::Aspect(4.0 / 3.0),
                LayoutTarget::Aspect(1.0),
            ]))
            .child(row(&[LayoutTarget::Row, LayoutTarget::Column]))
            .child(widgets::hint_text(self.language.layout_hint(), theme))
            .child(widgets::hint_text(self.language.drag_layout_hint(), theme))
    }

    fn gap_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = self.project.settings.gap;
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(widgets::section_title(self.language.image_gap(), theme))
            .child(widgets::switch_row(
                "toggle-gap",
                self.language.add_gap(),
                gap.enabled,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_gap_enabled(!this.project.settings.gap.enabled, cx);
                }),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .text_color(theme.text)
                    .child(self.language.gap_width())
                    .child(widgets::stepper(
                        "gap-px",
                        SharedString::from(format!("{} px", gap.px)),
                        52.0,
                        theme,
                        cx,
                        Self::step_gap_px,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.text)
                            .child(self.language.gap_color()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(widgets::color_dot(
                                "gap-color-black",
                                Rgba::BLACK,
                                gap.color == Rgba::BLACK,
                                theme,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.set_gap_color(Rgba::BLACK, cx);
                                }),
                            ))
                            .child(widgets::color_dot(
                                "gap-color-white",
                                Rgba::WHITE,
                                gap.color == Rgba::WHITE,
                                theme,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.set_gap_color(Rgba::WHITE, cx);
                                }),
                            ))
                            .child(widgets::color_dot(
                                "gap-color-grey",
                                Rgba::GREY,
                                gap.color == Rgba::GREY,
                                theme,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.set_gap_color(Rgba::GREY, cx);
                                }),
                            )),
                    ),
            )
    }

    fn window_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let section = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(widgets::section_title(self.language.window(), theme))
            .child(widgets::switch_row(
                "toggle-close-to-tray",
                self.language.close_to_tray(),
                self.close_to_tray,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_close_to_tray(!this.close_to_tray, cx);
                }),
            ))
            .child(widgets::hint_text(self.language.close_hint(), theme));
        #[cfg(target_os = "linux")]
        let section = section.child(widgets::hint_text(self.language.linux_tray_hint(), theme));
        section
    }

    fn language_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().items_center().gap(px(6.)).w_full();
        for language in Language::ALL {
            row = row.child(widgets::chip(
                SharedString::from(format!("language-{}", language.key())),
                language.language_name(),
                self.language == language,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.set_language(language, window, cx);
                }),
            ));
        }

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(widgets::section_title(self.language.language(), theme))
            .child(row)
    }

    fn selection_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .selected
            .and_then(|id| self.project.items.iter().find(|item| item.id == id));

        let mut section = div().flex().flex_col().gap(px(8.)).w_full();
        section = section.child(widgets::section_title(
            self.language.selected_image(),
            theme,
        ));

        match selected {
            Some(item) => {
                let id = item.id;
                let name = item.name.clone();
                let size = format!("{}×{}", item.width, item.height);
                let manual = item.manual;
                let keep_whole = item.keep_whole && manual.is_none();
                let status_text = if manual.is_some() {
                    self.language.manual_crop().to_string()
                } else {
                    match &item.status {
                        ItemStatus::Ready => match &item.block {
                            Some(block) => {
                                let lines = block.lines.len();
                                if lines >= 2 {
                                    self.language.found_lines(lines)
                                } else {
                                    self.language.found_subtitles().to_string()
                                }
                            }
                            None => self.language.whole_image().to_string(),
                        },
                        ItemStatus::NoSubtitle => self.language.no_subtitles().to_string(),
                        ItemStatus::Pending => self.language.detecting().to_string(),
                        ItemStatus::Failed(err) => self.language.read_failed(err),
                    }
                };
                let editing = self.crop_edit == Some(id);
                let selected_count = self.selected_count();
                let position = if manual.is_some() || editing {
                    self.active_manual_band()
                } else {
                    None
                };
                let position_text = position
                    .map(|band| {
                        self.language.manual_position(
                            (band.top * 100.0).round() as u32,
                            (band.bottom * 100.0).round() as u32,
                        )
                    })
                    .unwrap_or_default();

                section =
                    section
                        .child(div().text_sm().text_color(theme.text).child(name))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted)
                                .child(format!("{size} · {status_text}")),
                        )
                        .child(div().flex().items_center().gap(px(8.)).child(
                            widgets::tool_button(
                                "delete-selected",
                                self.language.delete_this(),
                                theme,
                                true,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.delete_selected_image(cx);
                                }),
                            ),
                        ));

                if self.project.mode == Mode::Subtitle && self.project.settings.crop {
                    section = section
                        .child(widgets::divider(theme))
                        .child(widgets::section_title(self.language.manual_crop(), theme));

                    if editing {
                        section = section.child(widgets::tool_button(
                            "manual-done",
                            self.language.manual_done(),
                            theme,
                            true,
                            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                this.finish_manual_edit(cx);
                            }),
                        ));
                        if position.is_some() {
                            section = section.child(widgets::hint_text(position_text, theme));
                        }
                        if manual.is_some() {
                            section = section.child(widgets::tool_button(
                                "manual-clear",
                                self.language.clear_manual(),
                                theme,
                                true,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.clear_manual_band(cx);
                                }),
                            ));
                        }
                        section = section.child(div().flex().items_center().gap(px(8.)).child(
                            widgets::tool_button(
                                "manual-apply-selected",
                                self.language.apply_to_selected(selected_count),
                                theme,
                                selected_count > 1,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.apply_manual_to_selected(cx);
                                }),
                            ),
                        ));
                        section = section.child(div().flex().items_center().gap(px(8.)).child(
                            widgets::tool_button(
                                "manual-apply-all",
                                self.language.apply_to_all(),
                                theme,
                                !self.project.items.is_empty(),
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.apply_manual_to_all(cx);
                                }),
                            ),
                        ));
                    } else {
                        section = section.child(widgets::tool_button(
                            "manual-start",
                            self.language.manual_edit(),
                            theme,
                            !matches!(item.status, ItemStatus::Pending | ItemStatus::Failed(_)),
                            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                this.begin_manual_edit(cx);
                            }),
                        ));
                        if manual.is_some() {
                            section = section.child(widgets::hint_text(position_text, theme));
                            section = section.child(widgets::tool_button(
                                "manual-clear",
                                self.language.clear_manual(),
                                theme,
                                true,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.clear_manual_band(cx);
                                }),
                            ));
                        }
                    }

                    section = section
                        .child(widgets::switch_row(
                            "manual-keep-whole",
                            self.language.keep_whole_image(),
                            keep_whole,
                            theme,
                            cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                this.set_selected_keep_whole(!keep_whole, cx);
                            }),
                        ))
                        .child(widgets::hint_text(self.language.manual_crop_hint(), theme));

                    if selected_count > 1 {
                        section = section.child(widgets::hint_text(
                            self.language.selected_count(selected_count),
                            theme,
                        ));
                    }
                }
            }
            None => {
                section = section.child(widgets::hint_text(self.language.selection_hint(), theme));
            }
        }

        if !self.project.items.is_empty() {
            section = section.child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .font_weight(FontWeight::NORMAL)
                    .child(self.language.image_count(self.project.items.len())),
            );
        }

        section
    }
}
