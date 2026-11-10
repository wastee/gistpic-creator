//! 通用界面小部件。
//!
//! 右键卡片、拖拽预览这些跨界面复用的零件放这里，具体界面自己组合。

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Div, ElementId, Hsla, IntoElement, SharedString, Stateful, Window, div, px,
};

use super::theme::Theme;
use super::tokens::{self, radius, size, space, text_size};

/// 把模型里的颜色转成界面颜色。
pub fn quad_color(color: crate::model::Rgba) -> Hsla {
    gpui::rgb(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32).into()
}

/// 菜单里的一行：左边是文字，右边可以带一个快捷键或状态提示。
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
        .text_color(color)
        .child(div().child(label.into()));
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
            .on_click(on_click)
    } else {
        row
    }
}

/// 菜单里带开关的一行。开关不用第三方组件，直接用两个圆角矩形画出来。
pub fn menu_switch_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
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
        .justify_between()
        .w_full()
        .h(size::MENU_ROW)
        .px(space::MD)
        .rounded(radius::SMALL)
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::BODY)
        .text_color(theme.text)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .on_click(on_click)
        .child(div().child(label.into()))
        .child(
            div()
                .flex()
                .items_center()
                .w(px(28.))
                .h(px(16.))
                .px(px(2.))
                .rounded(px(8.))
                .bg(if on { theme.accent } else { theme.track })
                .when(on, |this| this.justify_end())
                .child(
                    div()
                        .w(px(12.))
                        .h(px(12.))
                        .rounded(px(6.))
                        .bg(theme.surface),
                ),
        )
}

pub fn faint_text(label: impl Into<SharedString>, theme: Theme) -> impl IntoElement {
    div()
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::CAPTION)
        .text_color(theme.faint)
        .child(label.into())
}

pub fn divider(theme: Theme) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(theme.border_subtle).my(px(4.))
}
