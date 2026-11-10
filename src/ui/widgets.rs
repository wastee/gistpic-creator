//! 通用界面小部件。
//!
//! 全部遵守同一条规则：不用边框线区分，靠底色深浅；每个能点的东西按下时都有反馈。

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, IntoElement, Render, SharedString,
    Stateful, Window, div, px,
};

use super::icon::{self, Icon};
use super::theme::Theme;
use super::tokens::{self, radius, size, space, text_size};

/// 普通文字按钮：无底色无边框，悬停浮出一层浅灰。
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if enabled { theme.text } else { theme.faint };
    let element_id: ElementId = id.into();
    let debug_key = element_id.to_string();
    div()
        .debug_selector(move || debug_key.clone())
        .id(element_id)
        .flex()
        .items_center()
        .justify_center()
        .h(size::BUTTON)
        .px(space::MD)
        .rounded(radius::SMALL)
        .text_size(text_size::BODY)
        .font_family(tokens::FONT_FAMILY)
        .text_color(color)
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(theme.hover))
                .active(move |style| style.bg(theme.selected))
                .on_click(on_click)
        })
        .child(label.into())
}

/// 主按钮。整个界面只有导出用它。
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let text_color = if enabled {
        theme.accent_text
    } else {
        theme.accent_text.opacity(0.55)
    };
    let element_id: ElementId = id.into();
    let debug_key = element_id.to_string();
    div()
        .debug_selector(move || debug_key.clone())
        .id(element_id)
        .flex()
        .items_center()
        .justify_center()
        .h(size::BUTTON)
        .px(space::LG)
        .rounded(radius::SMALL)
        .bg(if enabled {
            theme.accent
        } else {
            theme.accent.opacity(0.45)
        })
        .text_size(text_size::BODY)
        .font_family(tokens::FONT_FAMILY)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text_color)
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.opacity(0.88))
                .active(move |style| style.opacity(0.75))
                .on_click(on_click)
        })
        .child(label.into())
}

/// 纯图标按钮。
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if enabled { theme.muted } else { theme.faint };
    let element_id: ElementId = id.into();
    let debug_key = element_id.to_string();
    div()
        .debug_selector(move || debug_key.clone())
        .id(element_id)
        .flex()
        .items_center()
        .justify_center()
        .w(size::ICON_BUTTON)
        .h(size::ICON_BUTTON)
        .rounded(radius::SMALL)
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(theme.hover))
                .active(move |style| style.bg(theme.selected))
                .on_click(on_click)
        })
        .child(icon::icon(glyph, px(16.), color))
}

/// 分段选择里的一个选项：选中时用选中底色，不用彩色。
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id: ElementId = id.into();
    let debug_key = element_id.to_string();
    div()
        .debug_selector(move || debug_key.clone())
        .id(element_id)
        .flex()
        .items_center()
        .justify_center()
        .h(size::ICON_BUTTON)
        .px(space::MD)
        .rounded(radius::SMALL)
        .bg(if active {
            theme.selected
        } else {
            gpui::transparent_black()
        })
        .text_size(text_size::BODY)
        .font_family(tokens::FONT_FAMILY)
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if active { theme.text } else { theme.muted })
        .cursor_pointer()
        .hover(move |style| {
            if active {
                style
            } else {
                style.bg(theme.hover)
            }
        })
        .active(move |style| style.opacity(0.7))
        .child(label.into())
        .on_click(on_click)
}

/// 一行开关：左边文字，右边轨道。
pub fn switch_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let track_bg = if checked { theme.text } else { theme.track };
    let knob = if checked { theme.bg } else { theme.muted };

    let mut track = div()
        .w(px(32.))
        .h(px(18.))
        .rounded(px(9.))
        .bg(track_bg)
        .flex()
        .items_center()
        .px(px(2.));
    track = if checked {
        track.justify_end()
    } else {
        track.justify_start()
    };
    track = track.child(div().w(px(14.)).h(px(14.)).rounded(px(7.)).bg(knob));

    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(size::BUTTON)
        .px(space::SM)
        .rounded(radius::SMALL)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .active(move |style| style.bg(theme.selected))
        .child(
            div()
                .font_family(tokens::FONT_FAMILY)
                .text_size(text_size::BODY)
                .text_color(theme.text)
                .child(label.into()),
        )
        .child(track)
        .on_click(on_click)
}

/// 点击式滑条：整条分成若干段，点哪段取哪个值。
pub fn segment_slider<T: 'static>(
    id_prefix: &'static str,
    active_index: usize,
    segments: usize,
    theme: Theme,
    cx: &mut Context<T>,
    on_pick: fn(&mut T, usize),
) -> Div {
    let segments = segments.max(2);
    let active_index = active_index.min(segments - 1);
    let mut row = div().flex().flex_row().items_center().gap(px(2.)).w_full();

    for index in 0..segments {
        let filled = index <= active_index;
        let listener = cx.listener(move |this: &mut T, _: &ClickEvent, _window, cx| {
            on_pick(this, index);
            cx.notify();
        });
        row = row.child(
            div()
                .id(SharedString::from(format!("{id_prefix}-seg-{index}")))
                .flex_1()
                .h(px(8.))
                .rounded(px(4.))
                .bg(if filled { theme.text } else { theme.track })
                .cursor_pointer()
                .hover(move |style| style.opacity(0.75))
                .active(move |style| style.opacity(0.6))
                .on_click(listener),
        );
    }

    row
}

/// 数值步进器：− 值 +。三个部分拼在一起，像一块整体。
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
        .gap(px(2.))
        .child(icon_button(
            SharedString::from(format!("{id_prefix}-minus")),
            Icon::Minus,
            theme,
            true,
            minus,
        ))
        .child(
            div()
                .w(px(width))
                .flex()
                .justify_center()
                .font_family(tokens::FONT_FAMILY)
                .text_size(text_size::BODY)
                .text_color(theme.text)
                .child(value_text),
        )
        .child(icon_button(
            SharedString::from(format!("{id_prefix}-plus")),
            Icon::Plus,
            theme,
            true,
            plus,
        ))
}

/// 小节标题。
pub fn section_title(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::TITLE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(text.into())
}

/// 一行说明文字。
pub fn hint_text(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::CAPTION)
        .text_color(theme.muted)
        .child(text.into())
}

/// 极淡的提示文字。
pub fn faint_text(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::CAPTION)
        .text_color(theme.faint)
        .child(text.into())
}

/// 水平分隔线，用一档底色差代替线。
pub fn divider(theme: Theme) -> Div {
    div().h(px(1.)).w_full().bg(theme.track.opacity(0.6))
}

/// 把模型里的颜色转成界面颜色。
pub fn quad_color(color: crate::model::Rgba) -> Hsla {
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
        .w(px(24.))
        .h(px(24.))
        .rounded(px(12.))
        .bg(quad_color(color))
        .when(selected, |this| {
            // 选中时外面套一圈：用一个更大的半透明环表示。
            this.border_2().border_color(theme.text)
        })
        .when(!selected, |this| {
            this.border_1().border_color(theme.track)
        })
        .cursor_pointer()
        .hover(move |style| style.opacity(0.85))
        .active(move |style| style.opacity(0.65))
        .on_click(on_click)
}

/// 菜单里的一行。
pub fn menu_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    enabled: bool,
    destructive: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if !enabled {
        theme.faint
    } else if destructive {
        theme.danger
    } else {
        theme.text
    };

    let element_id: ElementId = id.into();
    let debug_key = element_id.to_string();
    let row = div()
        .debug_selector(move || debug_key.clone())
        .id(element_id)
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(size::MENU_ROW)
        .px(space::MD)
        .rounded(radius::SMALL)
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::BODY)
        .text_color(color);

    let row = row.child(div().child(label.into()));

    let row = match hint {
        Some(hint) => row.child(
            div()
                .text_size(text_size::CAPTION)
                .text_color(theme.faint)
                .child(hint),
        ),
        None => row,
    };

    if enabled {
        row.cursor_pointer()
            .hover(move |style| style.bg(theme.hover))
            .active(move |style| style.bg(theme.selected))
            .on_click(on_click)
    } else {
        row
    }
}

/// 拖拽时跟随鼠标的预览块。
pub struct DragGhost {
    pub label: SharedString,
    pub theme: Theme,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(space::MD)
            .py(space::SM)
            .rounded(radius::SMALL)
            .bg(self.theme.surface)
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(self.theme.muted)
            .child(self.label.clone())
    }
}
