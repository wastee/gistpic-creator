//! 通用界面小部件。
//!
//! 右键卡片、拖拽预览这些跨界面复用的零件放这里，具体界面自己组合。

use gpui::prelude::*;
use gpui::{App, ClickEvent, ElementId, Hsla, IntoElement, SharedString, Window, div, px};

use super::hover::{self};
use super::theme::Theme;
use super::tokens::{self, radius, size, space, text_size};

/// 把模型里的颜色转成界面颜色。
pub fn quad_color(color: crate::model::Rgba) -> Hsla {
    gpui::rgb(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32).into()
}

/// 菜单行的状态：置灰不可点、普通可点，还是删除这类警示色。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuRowTone {
    /// 整行置灰，不可点击。
    Disabled,
    /// 可点击，普通文字色。
    Normal,
    /// 可点击，警示色，用于删除这类操作。
    Destructive,
}

impl MenuRowTone {
    fn enabled(self) -> bool {
        !matches!(self, MenuRowTone::Disabled)
    }

    fn text_color(self, theme: Theme) -> Hsla {
        match self {
            MenuRowTone::Disabled => theme.faint,
            MenuRowTone::Destructive => theme.danger,
            MenuRowTone::Normal => theme.text,
        }
    }
}

/// 菜单里的一行：左边是文字，右边可以带一个快捷键或状态提示。
///
/// 悬停反馈即时生效：`hover` 带着这行现在的激活状态和上报闭包，
/// 由调用方用 `hover_binding` 造出来。
pub fn menu_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    tone: MenuRowTone,
    theme: Theme,
    hover: hover::HoverBinding,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let enabled = tone.enabled();
    let element_id: ElementId = id.into();
    let selector_key = element_id.to_string();
    let row = div()
        .debug_selector(move || selector_key.clone())
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
        .text_color(tone.text_color(theme))
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
        let hoverable = row
            .bg(hover::fade_background(None, theme.hover, hover.fade))
            .cursor_pointer()
            .on_hover(hover.on_event)
            .on_click(on_click);
        hoverable.into_any_element()
    } else {
        row.into_any_element()
    }
}

/// 菜单里带开关的一行。开关不用第三方组件，直接用两个圆角矩形画出来。
///
/// 悬停反馈同样即时生效，参数含义见 [`menu_row`]。
pub fn menu_switch_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    theme: Theme,
    hover: hover::HoverBinding,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let element_id: ElementId = id.into();
    let selector_key = element_id.to_string();
    let row = div()
        .debug_selector(move || selector_key.clone())
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
        .on_hover(hover.on_event)
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
        );
    row.bg(hover::fade_background(None, theme.hover, hover.fade))
        .into_any_element()
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
