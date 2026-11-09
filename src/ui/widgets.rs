//! 通用界面小部件：按钮、开关、分段选择、滑条、步进器、拖拽预览。

use gpui::prelude::*;
use gpui::{
    div, px, App, ClickEvent, Context, Div, ElementId, FontWeight, IntoElement, Render,
    SharedString, Stateful, Window,
};

use super::theme::Theme;

/// 普通工具按钮。
pub fn tool_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.panel)
        .text_sm()
        .text_color(if enabled { theme.text } else { theme.muted })
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .child(label.into())
        .on_click(on_click)
}

/// 主按钮（强调色）。
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(12.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.accent)
        .bg(theme.accent)
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if enabled {
            theme.accent_text
        } else {
            theme.accent_text.opacity(0.6)
        })
        .cursor_pointer()
        .hover(move |style| style.opacity(0.9))
        .child(label.into())
        .on_click(on_click)
}

/// 小方形按钮，用于 + / − 之类。
pub fn icon_button(
    id: impl Into<ElementId>,
    label: &'static str,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(26.))
        .h(px(26.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.panel)
        .text_sm()
        .text_color(theme.text)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .child(label)
        .on_click(on_click)
}

/// 分段选择里的一个选项（选中时用强调色）。
pub fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (background, text_color, border) = if active {
        (theme.accent, theme.accent_text, theme.accent)
    } else {
        (theme.panel, theme.muted, theme.border)
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(26.))
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border)
        .bg(background)
        .text_sm()
        .text_color(text_color)
        .cursor_pointer()
        .hover(move |style| {
            if active {
                style.opacity(0.9)
            } else {
                style.bg(theme.hover)
            }
        })
        .child(label.into())
        .on_click(on_click)
}

/// 整行可点击的开关。
pub fn switch_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let mut track = div()
        .w(px(34.))
        .h(px(20.))
        .rounded(px(10.))
        .bg(if checked { theme.accent } else { theme.track })
        .flex()
        .items_center()
        .px(px(2.));
    track = if checked {
        track.justify_end()
    } else {
        track.justify_start()
    };
    track = track.child(
        div()
            .w(px(16.))
            .h(px(16.))
            .rounded(px(8.))
            .bg(gpui::white()),
    );

    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .py(px(4.))
        .px(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .child(div().text_sm().text_color(theme.text).child(label.into()))
        .child(track)
        .on_click(on_click)
}

/// 点击式滑条：整条分成 21 段，点哪段就取哪个值。
pub fn slider<T: 'static>(
    id_prefix: &'static str,
    ratio: f32,
    segments: usize,
    theme: Theme,
    cx: &mut Context<T>,
    on_pick: fn(&mut T, usize),
) -> Div {
    let segments = segments.max(2);
    let ratio = ratio.clamp(0.0, 1.0);
    let mut row = div().flex().flex_row().items_center().gap(px(2.)).w_full();

    for index in 0..segments {
        let filled = (index as f32 / (segments - 1) as f32) <= ratio + 1e-4;
        let listener = cx.listener(move |this: &mut T, _: &ClickEvent, _window, cx| {
            on_pick(this, index);
            cx.notify();
        });
        row = row.child(
            div()
                .id(SharedString::from(format!("{id_prefix}-seg-{index}")))
                .flex_1()
                .h(px(12.))
                .rounded(px(3.))
                .bg(if filled { theme.accent } else { theme.track })
                .cursor_pointer()
                .on_click(listener),
        );
    }

    row
}

/// 数值步进器：− / 值 / +。
pub fn stepper<T: 'static>(
    id_prefix: &'static str,
    value_text: SharedString,
    width: f32,
    theme: Theme,
    cx: &mut Context<T>,
    on_step: fn(&mut T, i32),
) -> Div {
    let minus = cx.listener(move |this: &mut T, _: &ClickEvent, _window, cx| {
        on_step(this, -1);
        cx.notify();
    });
    let plus = cx.listener(move |this: &mut T, _: &ClickEvent, _window, cx| {
        on_step(this, 1);
        cx.notify();
    });

    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(icon_button(
            SharedString::from(format!("{id_prefix}-minus")),
            "−",
            theme,
            minus,
        ))
        .child(
            div()
                .w(px(width))
                .flex()
                .justify_center()
                .text_sm()
                .text_color(theme.text)
                .child(value_text),
        )
        .child(icon_button(
            SharedString::from(format!("{id_prefix}-plus")),
            "+",
            theme,
            plus,
        ))
}

/// 小节标题。
pub fn section_title(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(theme.muted)
        .child(text.into())
}

/// 一行说明文字。
pub fn hint_text(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.muted)
        .child(text.into())
}

/// 水平分隔线。
pub fn divider(theme: Theme) -> Div {
    div().h(px(1.)).w_full().bg(theme.border)
}

/// 竖直分隔线（放在横向工具栏里）。
pub fn divider_vertical(theme: Theme) -> Div {
    div().w(px(1.)).h(px(20.)).bg(theme.border)
}

/// 把模型里的颜色转成界面颜色。
pub fn quad_color(color: crate::model::Rgba) -> gpui::Hsla {
    gpui::rgb(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32).into()
}

/// 小圆形颜色选择按钮。
pub fn color_dot(
    id: impl Into<ElementId>,
    color: crate::model::Rgba,
    selected: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(22.))
        .h(px(22.))
        .rounded(px(11.))
        .bg(quad_color(color))
        .border_2()
        .border_color(if selected { theme.accent } else { theme.border })
        .cursor_pointer()
        .on_click(on_click)
}

/// 拖拽时跟随鼠标的预览块。
pub struct DragGhost {
    pub label: SharedString,
    pub theme: Theme,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(self.theme.accent)
            .bg(self.theme.panel)
            .text_sm()
            .text_color(self.theme.text)
            .child(self.label.clone())
    }
}
