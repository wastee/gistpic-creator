//! 右侧设置面板。

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, FontWeight, IntoElement, SharedString,
};

use crate::app::AppRoot;
use crate::model::{ItemStatus, LayoutTarget, Mode, Rgba, SearchRegion};

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
                "裁剪字幕",
                settings.crop,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_crop(!this.project.settings.crop, cx);
                }),
            ))
            .child(widgets::switch_row(
                "toggle-keep-second",
                "保留第二行字幕",
                settings.keep_second,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_keep_second(!this.project.settings.keep_second, cx);
                }),
            ))
            .child(widgets::section_title("字幕搜索范围", theme))
            .child(self.region_row(
                theme,
                cx,
                &[SearchRegion::Auto, SearchRegion::Top],
            ))
            .child(self.region_row(
                theme,
                cx,
                &[SearchRegion::Bottom, SearchRegion::Whole],
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
                            .child("出血位")
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
            .child(widgets::hint_text(
                "出血位是字幕上下多留的一点边，避免把字裁掉。",
                theme,
            ));

        if !self.project.items.is_empty() {
            section = section.child(widgets::hint_text(
                format!("已识别到字幕 {detected} 张，没找到字幕 {missing} 张"),
                theme,
            ));
        }

        if settings.crop {
            section = section.child(widgets::hint_text(
                "第一张图保留画面主体：字幕在下方时保留上半部分，在字幕在上方时保留下半部分；其余图片只保留字幕那条。",
                theme,
            ));
        }

        section
    }

    fn region_row(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
        regions: &[SearchRegion],
    ) -> impl IntoElement {
        let current = self.project.settings.region;
        let mut row = div().flex().items_center().gap(px(6.)).w_full();
        for region in regions {
            let region = *region;
            row = row.child(widgets::chip(
                SharedString::from(format!("region-{}", region.label())),
                region.label(),
                current == region,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_region(region, cx);
                }),
            ));
        }
        row
    }

    fn layout_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.project.settings.layout;
        let row = |presets: &[LayoutTarget]| {
            let mut row = div().flex().items_center().gap(px(6.)).w_full();
            for target in presets {
                let target = *target;
                row = row.child(widgets::chip(
                    SharedString::from(format!("layout-{}", target.label())),
                    target.label(),
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
            .child(widgets::section_title("排列方式", theme))
            .child(row(&[
                LayoutTarget::Aspect(16.0 / 9.0),
                LayoutTarget::Aspect(9.0 / 16.0),
            ]))
            .child(row(&[
                LayoutTarget::Aspect(4.0 / 3.0),
                LayoutTarget::Aspect(1.0),
            ]))
            .child(row(&[LayoutTarget::Row, LayoutTarget::Column]))
            .child(widgets::hint_text(
                "按图片宽高自动分组排列，可能不按粘贴顺序；画布紧贴图片，不裁剪、不拉伸。想严格按顺序排，请用下面的横向或竖向排列。",
                theme,
            ))
            .child(widgets::hint_text(
                "可以拖动任意图片到另一张图片上，调整它所在的位置。",
                theme,
            ))
    }

    fn gap_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let gap = self.project.settings.gap;
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(widgets::section_title("图片缝隙", theme))
            .child(widgets::switch_row(
                "toggle-gap",
                "图片之间留缝",
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
                    .child("缝隙宽度")
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
                            .child("缝隙颜色"),
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
            .child(widgets::section_title("窗口", theme))
            .child(widgets::switch_row(
                "toggle-close-to-taskbar",
                "关闭窗口时最小化到任务栏",
                self.close_to_taskbar,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_close_to_taskbar(!this.close_to_taskbar, cx);
                }),
            ))
            .child(widgets::hint_text(
                "开着：点关闭按钮只是最小化，程序继续运行；关掉：点关闭按钮就退出。托盘图标可以把窗口叫回来。",
                theme,
            ));
        #[cfg(target_os = "linux")]
        let section = section.child(widgets::hint_text(
            "Linux 的托盘需要系统支持；GNOME 需要启用 AppIndicator 扩展才能看到图标。",
            theme,
        ));
        section
    }

    fn selection_settings(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .selected
            .and_then(|id| self.project.items.iter().find(|item| item.id == id));

        let mut section = div().flex().flex_col().gap(px(8.)).w_full();
        section = section.child(widgets::section_title("选中的图片", theme));

        match selected {
            Some(item) => {
                let status_text = match &item.status {
                    ItemStatus::Ready => match &item.block {
                        Some(block) => {
                            let lines = block.lines.len();
                            if lines >= 2 {
                                format!("已找到字幕（{lines} 行）")
                            } else {
                                "已找到字幕".to_string()
                            }
                        }
                        None => "整张图保留".to_string(),
                    },
                    ItemStatus::NoSubtitle => "没找到字幕".to_string(),
                    ItemStatus::Pending => "正在识别…".to_string(),
                    ItemStatus::Failed(err) => format!("读取失败：{err}"),
                };
                let name = item.name.clone();
                let size = format!("{}×{}", item.width, item.height);
                let id = item.id;

                section = section
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted)
                            .child(format!("{size} · {status_text}")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(widgets::tool_button(
                                "delete-selected",
                                "删除这张",
                                theme,
                                true,
                                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.delete_selected_image(cx);
                                }),
                            )),
                    );
                let _ = id;
            }
            None => {
                section = section.child(widgets::hint_text(
                    "在中间的预览里点一张图片，就能查看它、删除它。",
                    theme,
                ));
            }
        }

        if !self.project.items.is_empty() {
            section = section.child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .font_weight(FontWeight::NORMAL)
                    .child(format!("列表里一共 {} 张图片", self.project.items.len())),
            );
        }

        section
    }
}
