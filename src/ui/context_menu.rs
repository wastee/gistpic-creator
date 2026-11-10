//! 图片右键菜单：在预览区的某张图上点右键，弹出针对这张图的操作。

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, SharedString, div, px};

use crate::app::AppRoot;
use crate::model::{ItemId, ItemStatus};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};
use crate::ui::widgets;

impl AppRoot {
    pub(crate) fn render_image_menu(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = self.image_menu else {
            return div().into_any_element();
        };
        let id = menu.id;

        let Some(item) = self.project.items.iter().find(|item| item.id == id) else {
            return div().into_any_element();
        };

        let name = item.name.clone();
        let size = format!("{}×{}", item.width, item.height);
        let status_text = match &item.status {
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
        };
        let has_manual = item.manual.is_some();
        let keep_whole = item.keep_whole && item.manual.is_none();
        let can_crop = !matches!(item.status, ItemStatus::Pending | ItemStatus::Failed(_))
            && item.width > 0
            && item.height > 0;
        let selected_count = self.selected_count();
        let position = self
            .project
            .items
            .iter()
            .position(|item| item.id == id)
            .unwrap_or(0);
        let total = self.project.items.len();
        let can_move = total > 1;
        let selected_ids: Vec<ItemId> = self.selected_ids.iter().copied().collect();

        let mut menu_body = div()
            .id("image-menu")
            .w(size::MENU)
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(space::SM)
            .rounded(radius::LARGE)
            .bg(theme.surface)
            // 菜单左上角定位到右键点的位置
            .child(widgets::menu_row(
                "img-zoom",
                self.language.zoom_layer_action(),
                None,
                true,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.open_zoom_layer(id, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "img-crop",
                self.language.manual_crop_open(),
                None,
                can_crop,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.open_zoom_layer(id, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-reset-crop",
                self.language.reset_to_auto(),
                None,
                has_manual,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.select(id, cx);
                    this.clear_manual_band(cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-keep-whole",
                self.language.keep_whole_image(),
                Some(SharedString::from(if keep_whole {
                    self.language.on_label()
                } else {
                    self.language.off_label()
                })),
                can_crop,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.select(id, cx);
                    this.set_selected_keep_whole(!keep_whole, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-apply-crop",
                self.language.apply_to_selected(selected_count),
                None,
                has_manual && selected_count > 1,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.apply_manual_to_selected(cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "img-move-up",
                self.language.move_up(),
                None,
                can_move && position > 0,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.move_item(id, MoveDirection::Up, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-move-down",
                self.language.move_down(),
                None,
                can_move && position + 1 < total,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.move_item(id, MoveDirection::Down, cx);
                }),
            ))
            .child(widgets::menu_row(
                "img-move-last",
                self.language.move_to_last(),
                None,
                can_move && position + 1 < total,
                false,
                theme,
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.move_item(id, MoveDirection::Last, cx);
                }),
            ))
            .child(widgets::divider(theme))
            .child(
                // 详细信息：不占一行菜单，直接列在这里
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(space::MD)
                    .py(space::SM)
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY)
                            .text_size(text_size::CAPTION)
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(widgets::faint_text(
                        SharedString::from(format!("{size} · {status_text}")),
                        theme,
                    ))
                    .child(widgets::faint_text(
                        self.language.image_position(position + 1, total),
                        theme,
                    )),
            )
            .child(widgets::divider(theme))
            .child(widgets::menu_row(
                "img-delete",
                if selected_ids.len() > 1 {
                    self.language.delete_selected_count(selected_ids.len())
                } else {
                    self.language.delete_this().to_string()
                },
                None,
                true,
                true,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.image_menu = None;
                    this.delete_selected_images(cx);
                }),
            ));

        // 点菜单外面关掉菜单
        menu_body = menu_body.on_mouse_down_out(cx.listener(
            |this: &mut AppRoot, _: &gpui::MouseDownEvent, _window, cx| {
                this.close_image_menu(cx);
            },
        ));

        div()
            .id("image-menu-layer")
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(menu.x))
                    .top(px(menu.y))
                    .child(menu_body),
            )
            .into_any_element()
    }

    /// 把一张图往上、往下或移到最后。
    fn move_item(
        &mut self,
        id: ItemId,
        direction: MoveDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.project.items.iter().position(|item| item.id == id) else {
            return;
        };
        let to = match direction {
            MoveDirection::Up => from.saturating_sub(1),
            MoveDirection::Down => (from + 1).min(self.project.items.len().saturating_sub(1)),
            MoveDirection::Last => self.project.items.len().saturating_sub(1),
        };
        if from == to {
            return;
        }
        self.record_undo();
        let item = self.project.items.remove(from);
        self.project.items.insert(to, item);
        self.preview_cache.clear();
        cx.notify();
    }
}

/// 右键菜单里移动图片的方向。
enum MoveDirection {
    Up,
    Down,
    Last,
}
