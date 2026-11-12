//! 右键卡片：图片卡片和画布卡片都在这里画。
//!
//! 卡片是暖色的圆角浮层，不抢画面的注意力；点卡片外面就关掉。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, MouseDownEvent, SharedString, Size, div, px};

use crate::app::{AppRoot, MenuKind, MenuState, MoveDirection};
use crate::ui::hover::{self, HoverKey};
use crate::ui::icon::Icon;
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};
use crate::ui::widgets::{self, MenuRowTone};

impl AppRoot {
    /// 语言二级菜单跟随悬停：指针进入“语言”一行的当帧就弹出，
    /// 指针移开后再等 `SUBMENU_CLOSE_GRACE` 才收起，给用户留出滑进子菜单的时间。
    pub(crate) fn update_language_submenu(&mut self, cx: &mut Context<Self>) {
        let canvas_menu_open = matches!(
            self.menu.as_ref(),
            Some(MenuState {
                kind: MenuKind::Canvas,
                ..
            })
        );
        if !canvas_menu_open {
            return;
        }
        let row = HoverKey::widget("language-menu");
        if self.hover_gate.is_active(&row) {
            self.language_submenu_closing = false;
            if !self.language_submenu_open {
                self.language_submenu_open = true;
                cx.notify();
            }
            return;
        }
        if !self.language_submenu_open {
            return;
        }
        if self.should_keep_language_submenu() {
            self.language_submenu_closing = false;
            return;
        }
        if self.language_submenu_closing {
            return;
        }
        self.language_submenu_closing = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(hover::SUBMENU_CLOSE_GRACE)
                .await;
            let _ = this.update(cx, |root, cx| {
                root.language_submenu_closing = false;
                if root.language_submenu_open && !root.should_keep_language_submenu() {
                    root.language_submenu_open = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 指针还沾着“语言”行或子菜单里的行时，二级菜单要保持打开。
    fn should_keep_language_submenu(&self) -> bool {
        self.hover_gate
            .is_engaged(&HoverKey::widget("language-menu"))
            || self.hover_gate.is_engaged(&HoverKey::widget("lang-zh"))
            || self.hover_gate.is_engaged(&HoverKey::widget("lang-en"))
    }

    pub(crate) fn render_context_menu(
        &mut self,
        theme: Theme,
        viewport: Size<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = self.menu.as_ref().copied() else {
            return div().into_any_element();
        };
        let max_x = (f32::from(viewport.width) - f32::from(size::MENU) - 8.0).max(8.0);
        let max_y = (f32::from(viewport.height) - 420.0).max(8.0);
        let x = menu.x.min(max_x).max(8.0);
        let y = menu.y.min(max_y).max(8.0);

        let body = match menu.kind {
            MenuKind::Image(id) => self.render_image_card(id, theme, cx),
            MenuKind::Canvas => self.render_canvas_card(theme, cx),
        };

        let show_language_submenu =
            self.language_submenu_open && matches!(menu.kind, MenuKind::Canvas);
        let submenu_on_right =
            x + f32::from(size::MENU) * 2.0 + 4.0 <= f32::from(viewport.width) - 8.0;
        let group_x = if show_language_submenu && !submenu_on_right {
            (x - f32::from(size::MENU) - 4.0).max(8.0)
        } else {
            x
        };
        let card = div()
            .debug_selector(|| "context-menu".to_string())
            .w(size::MENU)
            .flex()
            .flex_col()
            .p(space::XS)
            .rounded(radius::CARD)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_float())
            .child(body);
        let mut group = div()
            .debug_selector(|| "context-menu-group".to_string())
            .absolute()
            .left(px(group_x))
            .top(px(y))
            .flex()
            .items_start()
            .gap(px(4.))
            .on_mouse_down_out(cx.listener(
                |this: &mut AppRoot, _: &MouseDownEvent, _window, cx| {
                    this.close_menu(cx);
                },
            ));
        if show_language_submenu && !submenu_on_right {
            group = group
                .child(self.render_language_submenu(theme, cx))
                .child(card);
        } else {
            group = group.child(card);
            if show_language_submenu {
                group = group.child(self.render_language_submenu(theme, cx));
            }
        }
        div()
            .id("menu-layer")
            .debug_selector(|| "menu-layer".to_string())
            .absolute()
            .inset_0()
            .occlude()
            .child(group)
            .into_any_element()
    }

    fn render_image_card(
        &mut self,
        id: crate::model::ItemId,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = self.language;
        let Some(item) = self.project.items.iter().find(|item| item.id == id) else {
            return div().into_any_element();
        };
        let name = item.name.clone();
        let resolution = language.image_resolution(item.width, item.height);
        let lines = item
            .block
            .as_ref()
            .map(|block| block.lines.len())
            .unwrap_or(0);
        let status = language.image_status(&item.status, lines);
        let has_block = item.block.is_some();
        let has_manual = item.manual.is_some();
        // 开关反映这张图现在是不是整张画面输出，和实际输出用同一套判断。
        let keep_whole = self.item_keeps_whole_image(id);
        let position = self
            .project
            .items
            .iter()
            .position(|item| item.id == id)
            .unwrap_or(0);
        let total = self.project.items.len();
        let can_move = total > 1;
        let selected_count = self.selected_count();

        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(space::MD)
                    .pt(space::SM)
                    .pb(space::XS)
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::BODY)
                            .font_weight(tokens::weight::STRONG)
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::CAPTION)
                            .text_color(theme.muted)
                            .child(format!("{resolution} · {status}")),
                    )
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::CAPTION)
                            .text_color(theme.faint)
                            .child(language.image_position(position + 1, total)),
                    ),
            )
            .child(widgets::divider(theme));

        if has_block {
            card = card.child(widgets::menu_row(
                "img-keep-single",
                language.keep_single_line(),
                None,
                MenuRowTone::Normal,
                theme,
                self.hover_binding("img-keep-single", cx),
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_keep_lines(id, false, cx);
                }),
            ));
        }
        card = card.child(widgets::menu_switch_row(
            "img-keep-whole",
            language.keep_whole_image(),
            keep_whole,
            theme,
            self.hover_binding("img-keep-whole", cx),
            cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                this.set_keep_whole(id, !keep_whole, cx);
            }),
        ));
        let crop_tone = if has_manual {
            MenuRowTone::Normal
        } else {
            MenuRowTone::Disabled
        };
        let move_up_tone = if can_move && position > 0 {
            MenuRowTone::Normal
        } else {
            MenuRowTone::Disabled
        };
        let move_down_tone = if can_move && position + 1 < total {
            MenuRowTone::Normal
        } else {
            MenuRowTone::Disabled
        };
        card = card
            .child(widgets::menu_row(
                "img-apply-crop",
                language.sync_crop_to_all(),
                None,
                crop_tone,
                theme,
                self.hover_binding("img-apply-crop", cx),
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.sync_crop_to_all(id, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-reset-crop",
                language.reset_to_auto(),
                None,
                crop_tone,
                theme,
                self.hover_binding("img-reset-crop", cx),
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.reset_item_auto(id, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "img-move-up",
                language.move_up(),
                None,
                move_up_tone,
                theme,
                self.hover_binding("img-move-up", cx),
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.move_item(id, MoveDirection::Up, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-move-down",
                language.move_down(),
                None,
                move_down_tone,
                theme,
                self.hover_binding("img-move-down", cx),
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.move_item(id, MoveDirection::Down, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "img-delete",
                if selected_count > 1 {
                    language.delete_selected_count(selected_count)
                } else {
                    language.delete_this().to_string()
                },
                None,
                MenuRowTone::Destructive,
                theme,
                self.hover_binding("img-delete", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.delete_from_menu(cx);
                }),
            ));
        card.into_any_element()
    }

    fn render_canvas_card(&mut self, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = self.language;
        let close_to_tray = self.close_to_tray;
        let card = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(widgets::menu_row(
                "open-images",
                language.open_images(),
                Some(SharedString::from("Ctrl+O")),
                MenuRowTone::Normal,
                theme,
                self.hover_binding("open-images", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.close_menu(cx);
                    this.open_images(cx);
                }),
            ))
            .child(widgets::menu_row(
                "paste-image",
                language.paste_image_plain(),
                Some(SharedString::from("Ctrl+V")),
                MenuRowTone::Normal,
                theme,
                self.hover_binding("paste-image", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.close_menu(cx);
                    this.paste(cx);
                }),
            ))
            .child(widgets::menu_row(
                "clear-all",
                language.clear_all(),
                None,
                if self.project.items.is_empty() {
                    MenuRowTone::Disabled
                } else {
                    MenuRowTone::Destructive
                },
                theme,
                self.hover_binding("clear-all", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.clear_all(cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child({
                let fade = self
                    .hover_gate
                    .fade_delta(&HoverKey::widget("language-menu"));
                let language_row = div()
                    .debug_selector(|| "language-menu".to_string())
                    .id("language-menu")
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
                    .bg(hover::fade_background(None, theme.hover, fade))
                    .on_hover(self.widget_hover_listener("language-menu", cx))
                    .child(language.language())
                    .child(crate::ui::icon::icon(
                        Icon::ChevronRight,
                        px(14.),
                        theme.muted,
                    ));
                language_row.into_any_element()
            })
            .child(widgets::divider(theme))
            .child(
                div()
                    .debug_selector(|| "exit-section".to_string())
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(widgets::menu_switch_row(
                        "toggle-tray",
                        language.close_to_tray(),
                        close_to_tray,
                        theme,
                        self.hover_binding("toggle-tray", cx),
                        cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.set_close_to_tray(!close_to_tray, cx);
                        }),
                    ))
                    .child(widgets::menu_row(
                        "quit-app",
                        language.quit(),
                        Some(SharedString::from("Ctrl+Q")),
                        MenuRowTone::Normal,
                        theme,
                        self.hover_binding("quit-app", cx),
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                            this.request_quit(window, cx);
                        }),
                    )),
            );
        card.into_any_element()
    }

    fn render_language_submenu(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let current = self.language;
        div()
            .debug_selector(|| "language-submenu".to_string())
            .mt(px(78.))
            .w(size::MENU)
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(space::XS)
            .rounded(radius::CARD)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_float())
            .child(widgets::menu_row(
                "lang-zh",
                crate::i18n::Language::ZhCn.language_name(),
                Some(SharedString::from(
                    if matches!(current, crate::i18n::Language::ZhCn) {
                        "●"
                    } else {
                        ""
                    },
                )),
                MenuRowTone::Normal,
                theme,
                self.hover_binding("lang-zh", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.set_language(crate::i18n::Language::ZhCn, window, cx);
                    this.close_menu(cx);
                }),
            ))
            .child(widgets::menu_row(
                "lang-en",
                crate::i18n::Language::EnUs.language_name(),
                Some(SharedString::from(
                    if matches!(current, crate::i18n::Language::EnUs) {
                        "●"
                    } else {
                        ""
                    },
                )),
                MenuRowTone::Normal,
                theme,
                self.hover_binding("lang-en", cx),
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, window, cx| {
                    this.set_language(crate::i18n::Language::EnUs, window, cx);
                    this.close_menu(cx);
                }),
            ))
    }
}
