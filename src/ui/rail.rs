//! 右边缘图标条：平时只有一条窄栏，鼠标移上去展开参数面板，可以固定常开。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, SharedString, div, px};

use crate::app::AppRoot;
use crate::model::{LayoutTarget, Rgba};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};
use crate::ui::widgets;

/// 图标条里展开哪一块面板。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailPanel {
    /// 字幕参数
    Subtitle,
    /// 排版参数
    Layout,
    /// 图片缝隙
    Gap,
}

impl RailPanel {
    fn icon(self) -> Icon {
        match self {
            RailPanel::Subtitle => Icon::Subtitle,
            RailPanel::Layout => Icon::Layout,
            RailPanel::Gap => Icon::Gap,
        }
    }

    fn key(self) -> &'static str {
        match self {
            RailPanel::Subtitle => "subtitle",
            RailPanel::Layout => "layout",
            RailPanel::Gap => "gap",
        }
    }
}

impl AppRoot {
    /// 当前应该展开哪一块面板。没固定住且鼠标不在图标条上时返回 None。
    pub(crate) fn visible_rail_panel(&self) -> Option<RailPanel> {
        if self.rail_pinned {
            self.rail_active
        } else {
            self.rail_open
        }
    }

    pub(crate) fn render_rail(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.visible_rail_panel();

        // 图标条固定在右边缘，位置永不变动；参数面板浮在它左边、盖在图片上方。
        //
        // 面板一开始是用正常流排在图标条左边的，那样图标条会被往左挤 260 像素：
        // 鼠标虽然没动，相对图标条的坐标却变了，悬停立刻判定为「离开」，
        // 面板收起、图标条弹回原位、鼠标又命中，形成反复开合。
        div()
            .relative()
            .h_full()
            .flex_shrink_0()
            .child(self.rail_strip(theme, cx))
            .when_some(panel, |this, panel| {
                this.child(
                    div()
                        .absolute()
                        // 贴着图标条左边缘展开，不占正常布局空间。
                        .right(size::RAIL)
                        .top(px(0.))
                        .bottom(px(0.))
                        .child(self.rail_panel(panel, theme, cx)),
                )
            })
    }

    /// 窄栏本体：几个图标从下到上排。
    fn rail_strip(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let pinned = self.rail_pinned;
        let active = self.visible_rail_panel();
        // 面板是否已经展开。图钉只在展开后出现。
        let panel_open = active.is_some();

        let mut strip = div()
            .debug_selector(|| "rail-strip".to_string())
            .id("rail-strip")
            .w(size::RAIL)
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .py(space::SM)
            .bg(theme.bg);

        // 三个参数图标。排版模式下字幕图标用不上：点了会自动切回字幕模式再展开，
        // 这样点一下就能用，不用先想「为什么点不动」。
        for panel in [RailPanel::Subtitle, RailPanel::Layout, RailPanel::Gap] {
            let usable_in_mode = match panel {
                RailPanel::Subtitle => self.project.mode == crate::model::Mode::Subtitle,
                RailPanel::Layout => true,
                RailPanel::Gap => true,
            };
            let is_active = active == Some(panel);
            let color = if is_active {
                theme.text
            } else if !usable_in_mode {
                theme.faint
            } else {
                theme.muted
            };
            let rail_icon = panel.icon();
            let key = panel.key();

            strip = strip.child(
                div()
                    .debug_selector(move || format!("rail-{key}"))
                    .id(SharedString::from(format!("rail-{key}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(size::RAIL_ICON)
                    .h(size::RAIL_ICON)
                    .rounded(radius::SMALL)
                    .when(is_active, |this| this.bg(theme.selected))
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.hover))
                    .active(move |style| style.bg(theme.selected))
                    .on_click(cx.listener(
                        move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.open_rail_panel(panel, cx);
                        },
                    ))
                    .child(icon::icon(rail_icon, px(18.), color)),
            );
        }

        // 图钉只在面板已经展开时出现。面板还没展开时，图标条上只有三个参数图标。
        let pin_color = if pinned { theme.text } else { theme.muted };
        strip = strip
            .when(panel_open, |this| {
                this.child(div().h(space::SM)).child(
                    div()
                        .debug_selector(|| "rail-pin".to_string())
                        .id("rail-pin")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(size::RAIL_ICON)
                        .h(size::RAIL_ICON)
                        .rounded(radius::SMALL)
                        .when(pinned, |this| this.bg(theme.selected))
                        .cursor_pointer()
                        .hover(move |style| style.bg(theme.hover))
                        .active(move |style| style.bg(theme.selected))
                        .on_click(cx.listener(
                            |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                this.toggle_rail_pin(cx);
                            },
                        ))
                        .child(icon::icon(Icon::Pin, px(16.), pin_color)),
                )
            })
            // 鼠标离开整条时收面板（没固定的情况）
            .on_hover(cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                this.set_rail_hover(*hovered, cx);
            }));

        strip
    }

    /// 展开的参数面板。
    fn rail_panel(
        &mut self,
        panel: RailPanel,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let content = match panel {
            RailPanel::Subtitle => self.rail_subtitle(theme, cx).into_any_element(),
            RailPanel::Layout => self.rail_layout(theme, cx).into_any_element(),
            RailPanel::Gap => self.rail_gap(theme, cx).into_any_element(),
        };

        div()
            .debug_selector(|| "rail-panel".to_string())
            .id("rail-panel")
            .w(size::RAIL_PANEL)
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .child(
                div()
                    .id("rail-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(space::MD)
                    .p(space::LG)
                    .child(content),
            )
            .on_hover(cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                this.set_rail_hover(*hovered, cx);
            }))
    }

    fn rail_subtitle(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &self.project.settings;
        let bleed_index = tokens::bleed_to_index(settings.bleed);
        let bleed_percent = (settings.bleed * 100.0).round() as u32;

        let mut section = div()
            .flex()
            .flex_col()
            .gap(space::SM)
            .w_full()
            .child(widgets::switch_row(
                "rail-toggle-crop",
                self.language.crop_subtitles(),
                settings.crop,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_crop(!this.project.settings.crop, cx);
                }),
            ))
            .child(widgets::switch_row(
                "rail-toggle-keep-second",
                self.language.keep_second_line(),
                settings.keep_second,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_keep_second(!this.project.settings.keep_second, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(widgets::hint_text(self.language.bleed(), theme))
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::CAPTION)
                            .text_color(theme.text)
                            .child(format!("{bleed_percent}%")),
                    ),
            )
            .child(widgets::segment_slider(
                "rail-bleed",
                bleed_index,
                tokens::BLEED_STEPS,
                theme,
                cx,
                Self::set_bleed_index,
            ));

        // 有图片时才显示检测结果统计。
        if !self.project.items.is_empty() {
            let detected = self
                .project
                .items
                .iter()
                .filter(|item| item.status == crate::model::ItemStatus::Ready)
                .count();
            let missing = self
                .project
                .items
                .iter()
                .filter(|item| item.status == crate::model::ItemStatus::NoSubtitle)
                .count();
            section = section.child(widgets::faint_text(
                self.language.detected_summary(detected, missing),
                theme,
            ));
        }

        section
    }

    fn rail_layout(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.project.settings.layout;

        let target = |label: &'static str, target: LayoutTarget, theme: Theme, cx: &mut Context<AppRoot>| {
            widgets::segment(
                SharedString::from(format!("rail-layout-{}", target.key())),
                label,
                current == target,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_layout_target(target, cx);
                }),
            )
        };

        div()
            .flex()
            .flex_col()
            .gap(space::SM)
            .w_full()
            .child(widgets::section_title(self.language.arrangement(), theme))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(2.))
                    .w_full()
                    .child(target("16:9", LayoutTarget::Aspect(16.0 / 9.0), theme, cx))
                    .child(target("9:16", LayoutTarget::Aspect(9.0 / 16.0), theme, cx))
                    .child(target("4:3", LayoutTarget::Aspect(4.0 / 3.0), theme, cx))
                    .child(target("1:1", LayoutTarget::Aspect(1.0), theme, cx))
                    .child(target("横向", LayoutTarget::Row, theme, cx))
                    .child(target("竖向", LayoutTarget::Column, theme, cx)),
            )
            .child(widgets::faint_text(self.language.layout_hint(), theme))
    }

    fn rail_gap(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = self.project.settings.gap;

        div()
            .flex()
            .flex_col()
            .gap(space::SM)
            .w_full()
            .child(widgets::switch_row(
                "rail-toggle-gap",
                self.language.add_gap(),
                gap.enabled,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_gap_enabled(!this.project.settings.gap.enabled, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(widgets::hint_text(self.language.gap_width(), theme))
                    .child(widgets::stepper(
                        "rail-gap-px",
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
                    .w_full()
                    .child(widgets::hint_text(self.language.gap_color(), theme))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::SM)
                            .child(widgets::color_dot(
                                "rail-gap-black",
                                Rgba::BLACK,
                                gap.color == Rgba::BLACK,
                                theme,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.set_gap_color(Rgba::BLACK, cx);
                                }),
                            ))
                            .child(widgets::color_dot(
                                "rail-gap-white",
                                Rgba::WHITE,
                                gap.color == Rgba::WHITE,
                                theme,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.set_gap_color(Rgba::WHITE, cx);
                                }),
                            ))
                            .child(widgets::color_dot(
                                "rail-gap-grey",
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
}
