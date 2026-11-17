//! 右键卡片：图片卡片和画布卡片都在这里画。
//!
//! 卡片是暖色的圆角浮层，不抢画面的注意力；点卡片外面就关掉。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, MouseButton, MouseUpEvent, SharedString, anchored, div, px};

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

    /// 画右键卡片。菜单画在主窗口内，鼠标位置是窗口内坐标；
    /// 放不下时翻到鼠标另一侧，整张卡片始终留在主窗口里。
    pub(crate) fn render_context_menu(
        &mut self,
        theme: Theme,
        viewport: gpui::Size<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(menu) = self.menu.as_ref().copied() else {
            return div().into_any_element();
        };

        let body = match menu.kind {
            MenuKind::Image(id) => self.render_image_card(id, theme, cx),
            MenuKind::Canvas => self.render_canvas_card(theme, cx),
        };

        let show_language_submenu =
            self.language_submenu_open && matches!(menu.kind, MenuKind::Canvas);

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
            .child(body);

        // 二级菜单绝对定位在主卡片左右，不参与主卡片的尺寸测量。
        // 这样 anchored 永远按主卡片自己的宽度（216）定位，展开二级菜单不会
        // 让整组变宽而翻转，一级菜单的位置保持稳定，鼠标能稳稳停在语言行上。
        let vw = f32::from(viewport.width);
        let card_w = f32::from(size::MENU);
        let submenu_on_right = menu.x + card_w * 2.0 + 4.0 + 8.0 <= vw;
        let submenu_x = if submenu_on_right {
            px(card_w + 4.)
        } else {
            px(-card_w - 4.)
        };

        let mut card_with_sub = div().relative().child(card);
        if show_language_submenu {
            card_with_sub = card_with_sub.child(
                div()
                    .absolute()
                    .left(submenu_x)
                    .top(px(78.))
                    .child(self.render_language_submenu(theme, cx)),
            );
        }

        let group = div()
            .debug_selector(|| "context-menu-group".to_string())
            .flex()
            .items_start()
            .on_mouse_up_out(MouseButton::Left, cx.listener(
                |this: &mut AppRoot, event: &MouseUpEvent, window, cx| {
                    if crate::ui::window_controls::close_button_hit(
                        event.position,
                        window.viewport_size(),
                    ) {
                        this.on_close_requested(window, cx);
                        return;
                    }
                    this.close_menu(cx);
                },
            ))
            .child(card_with_sub);

        // anchored 用菜单实际测量出来的尺寸做定位：左上角先放在鼠标处，
        // 向右下展开；哪一边放不下就沿那一边翻转，让整张菜单贴着鼠标且
        // 完整留在窗口里。全平台统一走这套布局逻辑，不依赖操作系统弹窗。
        div()
            .id("menu-layer")
            .debug_selector(|| "menu-layer".to_string())
            .absolute()
            .inset_0()
            .occlude()
            // 菜单层铺满整个窗口，落在上面的外部文件同样需要转发给根节点。
            .on_drop(cx.listener(Self::on_file_drop))
            .child(
                anchored()
                    .anchor(gpui::Corner::TopLeft)
                    .position(gpui::point(gpui::px(menu.x), gpui::px(menu.y)))
                    .child(group),
            )
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
        let clear_after_copy_export = self.clear_after_copy_export;
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
                    .child(widgets::menu_switch_row(
                        "toggle-clear-after-copy-export",
                        language.clear_after_copy_export(),
                        clear_after_copy_export,
                        theme,
                        self.hover_binding("toggle-clear-after-copy-export", cx),
                        cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.set_clear_after_copy_export(!clear_after_copy_export, cx);
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
            .w(size::MENU)
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(space::XS)
            .rounded(radius::CARD)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
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
