//! 顶部工具栏：模式切换、撤销重做、导出，以及左上角的低频项菜单。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, div, px};

use crate::app::AppRoot;
use crate::app::actions::*;
use crate::model::{LayoutTarget, Mode};
use crate::ui::icon::{self, Icon};
use crate::ui::tokens::{self, radius, size, space, text_size};

use super::theme::Theme;
use super::widgets;

impl AppRoot {
    pub(crate) fn render_toolbar(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let can_undo = self.undo.can_undo();
        let can_redo = self.undo.can_redo();
        let has_images = !self.project.items.is_empty();

        let mode_switch = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(widgets::segment(
                "mode-subtitle",
                self.language.mode_label(Mode::Subtitle),
                self.project.mode == Mode::Subtitle,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(Mode::Subtitle, cx)
                }),
            ))
            .child(widgets::segment(
                "mode-layout",
                self.language.mode_label(Mode::Layout),
                self.project.mode == Mode::Layout,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(Mode::Layout, cx)
                }),
            ));

        div()
            .debug_selector(|| "toolbar".to_string())
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .h(size::TOOLBAR)
            .px(space::MD)
            .bg(theme.bg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::SM)
                    // 左侧：更多按钮 + 应用名
                    .child(
                        div()
                            .debug_selector(|| "toolbar-more".to_string())
                            .id("toolbar-more")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(size::ICON_BUTTON)
                            .h(size::ICON_BUTTON)
                            .rounded(radius::SMALL)
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.hover))
                            .active(move |style| style.bg(theme.selected))
                            .on_click(cx.listener(
                                |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                    this.toggle_more_menu(cx);
                                },
                            ))
                            .child(icon::icon(Icon::More, px(16.), theme.muted)),
                    )
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::CAPTION)
                            .text_color(theme.faint)
                            .child(self.language.app_name()),
                    )
                    .child(div().w(space::SM))
                    .child(mode_switch),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(widgets::icon_button(
                        "toolbar-undo",
                        Icon::Undo,
                        theme,
                        can_undo,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_undo(&UndoEdit, window, cx)
                        }),
                    ))
                    .child(widgets::icon_button(
                        "toolbar-redo",
                        Icon::Redo,
                        theme,
                        can_redo,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_redo(&RedoEdit, window, cx)
                        }),
                    ))
                    .child(div().w(space::SM))
                    .child(widgets::primary_button(
                        "export-png",
                        self.language.export_png(),
                        theme,
                        has_images,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.action_export(&ExportPng, window, cx)
                        }),
                    )),
            )
    }

    /// 左上角「更多」按钮弹出的小菜单：不常改的东西都在这里。
    pub(crate) fn render_more_menu(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_images = !self.project.items.is_empty();
        let language = self.language;
        let close_to_tray = self.close_to_tray;

        let mut menu = div()
            .id("more-menu")
            .w(px(220.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(space::SM)
            .rounded(radius::LARGE)
            .bg(theme.surface)
            .child(widgets::menu_row(
                "more-open",
                self.language.open_images(),
                Some(self.shortcut_hint("O")),
                true,
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.close_more_menu(cx);
                    this.action_open(&OpenImages, window, cx);
                }),
            ))
            .child(widgets::menu_row(
                "more-paste",
                self.language.paste_image_plain(),
                Some(self.shortcut_hint("V")),
                true,
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.close_more_menu(cx);
                    this.action_paste(&PasteImage, window, cx);
                }),
            ))
            .child(widgets::menu_row(
                "more-copy",
                self.language.copy_image(),
                Some(self.shortcut_hint("Shift+C")),
                has_images,
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.close_more_menu(cx);
                    this.action_copy(&CopyImage, window, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "more-clear",
                self.language.clear_all(),
                None,
                has_images,
                true,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.close_more_menu(cx);
                    this.action_clear(&ClearAll, window, cx);
                }),
            ))
            .child(widgets::divider(theme))
            // 语言：两个选项并排，当前语言加粗
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .h(size::MENU_ROW)
                    .px(space::MD)
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::BODY)
                            .text_color(theme.text)
                            .child(self.language.language()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .children(crate::i18n::Language::ALL.into_iter().map(|item| {
                                widgets::segment(
                                    gpui::SharedString::from(format!("more-lang-{}", item.key())),
                                    item.language_name(),
                                    language == item,
                                    theme,
                                    cx.listener(
                                        move |this: &mut AppRoot, _: &ClickEvent, window, cx| {
                                            this.set_language(item, window, cx);
                                            this.close_more_menu(cx);
                                        },
                                    ),
                                )
                            })),
                    ),
            )
            .child(widgets::menu_row(
                "more-close-to-tray",
                self.language.close_to_tray(),
                Some(gpui::SharedString::from(if close_to_tray {
                    self.language.on_label()
                } else {
                    self.language.off_label()
                })),
                true,
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_close_to_tray(!this.close_to_tray, cx);
                }),
            ))
            .child(
                div()
                    .px(space::MD)
                    .pb(space::SM)
                    .child(widgets::faint_text(self.language.close_hint(), theme)),
            )
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "more-quit",
                self.language.quit(),
                Some(self.shortcut_hint("Q")),
                true,
                false,
                theme,
                cx.listener(|_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    cx.quit();
                }),
            ));

        // 排版模式下拖角改比例的提示只在菜单底部出现一次。
        if self.project.mode == Mode::Layout
            && matches!(self.project.settings.layout, LayoutTarget::Aspect(_))
        {
            menu = menu.child(widgets::divider(theme)).child(
                div().px(space::MD).py(space::SM).child(widgets::faint_text(
                    self.language.corner_drag_hint(),
                    theme,
                )),
            );
        }

        menu
    }

    /// 更多菜单的浮层：一层透明遮罩 + 菜单本体。
    ///
    /// 这层必须画在根视图最上面，不能挂在工具栏容器里，否则会被后面铺满窗口的
    /// 预览区盖住，变成看不见内容、却能点到的状态。
    pub(crate) fn render_more_menu_layer(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .debug_selector(|| "more-menu-layer".to_string())
            .id("more-menu-layer")
            .absolute()
            .inset_0()
            // 点菜单外面的任何地方都关掉菜单。
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &gpui::MouseDownEvent, _window, cx| {
                    this.close_more_menu(cx);
                }),
            )
            .child(
                div()
                    .id("more-menu-anchor")
                    .absolute()
                    .left(space::MD)
                    .top(size::TOOLBAR - px(4.))
                    // 点菜单本体时不要把事件冒泡给上面的遮罩，否则菜单刚点就关。
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(
                            |_this: &mut AppRoot, _: &gpui::MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                            },
                        ),
                    )
                    .child(self.render_more_menu(theme, cx)),
            )
    }

    /// 快捷键提示文字，例如 `Ctrl+O`。
    fn shortcut_hint(&self, key: &str) -> gpui::SharedString {
        crate::platform::shortcut_label(key).into()
    }
}
