//! 底部单一灵动胶囊：模式、留白或排版预设、添加、撤销、复制、导出。
//!
//! 正常状态是一条窄胶囊；拖动图片时它变成底部的删除区。

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, DragMoveEvent, IntoElement, MouseButton, MouseDownEvent, ScrollWheelEvent,
    SharedString, div, px,
};

use crate::app::{AppRoot, DragItem};
use crate::model::{LayoutTarget, Mode};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};

impl AppRoot {
    pub(crate) fn render_capsule(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dragging = self.dragging.is_some();
        let mut column = div()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(size::CAPSULE_BOTTOM)
            .flex()
            .flex_col()
            .items_center()
            .gap(size::TOAST_GAP);

        if (self.toast_visible || self.detecting) && !dragging {
            column = column.child(self.render_toast(theme));
        }

        column.child(self.render_pill(theme, dragging, cx))
    }

    fn render_pill(
        &mut self,
        theme: Theme,
        dragging: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut pill = div()
            .debug_selector(|| "capsule".to_string())
            .id("capsule")
            .flex()
            .items_center()
            .gap(space::SM)
            .h(if dragging { px(52.) } else { size::CAPSULE })
            .px(px(if dragging { 24. } else { 10. }))
            .rounded(radius::PILL)
            .bg(if dragging {
                theme.danger
            } else {
                theme.surface
            })
            .border_1()
            .border_color(if dragging {
                theme.danger
            } else {
                theme.border_subtle
            })
            .shadow(theme.shadow_float())
            .occlude()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this: &mut AppRoot, event: &MouseDownEvent, _window, cx| {
                    this.open_canvas_menu(
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                        cx,
                    );
                }),
            );

        if dragging {
            let count = self.selected_count().max(1);
            let text = if self.delete_hover {
                self.language.delete_zone_hover().to_string()
            } else {
                self.language.delete_zone_idle(count)
            };
            pill = pill
                .debug_selector(|| "delete-zone".to_string())
                .cursor(gpui::CursorStyle::PointingHand)
                .on_drag_move::<DragItem>(cx.listener(
                    |this: &mut AppRoot, _: &DragMoveEvent<DragItem>, _window, cx| {
                        this.set_delete_hover(true, cx);
                    },
                ))
                .on_drop(
                    cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                        this.drop_on_delete(dragged.id, cx);
                    }),
                )
                .child(icon::icon(Icon::Trash, px(20.), theme.accent_text))
                .child(
                    div()
                        .font_family(tokens::FONT_FAMILY)
                        .text_size(text_size::BODY)
                        .font_weight(tokens::weight::STRONG)
                        .text_color(theme.accent_text)
                        .child(text),
                );
            return pill;
        }

        // 模式分段
        let mode = self.project.mode;
        pill = pill
            .child(self.mode_button(Mode::Subtitle, mode, theme, cx))
            .child(self.mode_button(Mode::Layout, mode, theme, cx))
            .child(widgets_divider(theme));

        match mode {
            Mode::Subtitle => {
                pill = pill
                    .child(self.render_bleed_slider(theme, cx))
                    .child(self.render_keep_second_toggle(theme, cx));
            }
            Mode::Layout => {
                pill = pill.child(self.render_layout_presets(theme, cx));
            }
        }

        pill = pill.child(widgets_divider(theme)).child(action_button(
            "capsule-add",
            Icon::Plus,
            self.language.add_images(),
            false,
            theme,
            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.open_images(cx)),
        ));
        if self.undo.can_undo() {
            pill = pill.child(action_button(
                "capsule-undo",
                Icon::Undo,
                "",
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.undo_edit(cx)),
            ));
        }
        pill = pill
            .child(action_button(
                "capsule-copy",
                Icon::Copy,
                self.language.copy_short(),
                false,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.copy_image(cx)),
            ))
            .child(action_button(
                "export-png",
                Icon::Export,
                self.language.export_png(),
                true,
                theme,
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.export_png(cx)),
            ));
        pill
    }

    fn mode_button(
        &mut self,
        mode: Mode,
        current: Mode,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = mode == current;
        let label = match mode {
            Mode::Subtitle => self.language.mode_label(Mode::Subtitle),
            Mode::Layout => self.language.mode_label(Mode::Layout),
        };
        div()
            .debug_selector(move || match mode {
                Mode::Subtitle => "mode-subtitle".to_string(),
                Mode::Layout => "mode-layout".to_string(),
            })
            .id(match mode {
                Mode::Subtitle => "mode-subtitle",
                Mode::Layout => "mode-layout",
            })
            .flex()
            .items_center()
            .justify_center()
            .h(size::CAPSULE_BUTTON)
            .px(px(11.))
            .rounded(radius::PILL)
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::BODY)
            .font_weight(if active {
                tokens::weight::STRONG
            } else {
                tokens::weight::REGULAR
            })
            .text_color(if active { theme.text } else { theme.muted })
            .cursor_pointer()
            .when(active, |this| this.bg(theme.surface_elevated))
            .hover(move |style| style.bg(theme.hover))
            .on_click(
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(mode, cx);
                }),
            )
            .child(label)
    }

    fn render_bleed_slider(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let index = tokens::bleed_to_index(self.project.settings.bleed);
        let language = self.language;
        let mut track = div()
            .debug_selector(|| "bleed-slider".to_string())
            .id("bleed-slider")
            .flex()
            .items_center()
            .gap(px(2.))
            .h(size::CAPSULE_BUTTON)
            .on_scroll_wheel(cx.listener(
                |this: &mut AppRoot, event: &ScrollWheelEvent, _window, cx| {
                    let delta = match event.delta {
                        gpui::ScrollDelta::Lines(point) => point.y,
                        gpui::ScrollDelta::Pixels(point) => f32::from(point.y) / 20.0,
                    };
                    if delta.abs() < f32::EPSILON {
                        return;
                    }
                    this.step_bleed(if delta > 0.0 { 1 } else { -1 }, cx);
                    cx.stop_propagation();
                },
            ))
            .on_drag(
                crate::app::BleedDrag,
                move |_value, _offset, _window, cx| cx.new(|_| CapsuleDragGhost),
            )
            .on_drag_move::<crate::app::BleedDrag>(cx.listener(
                |this: &mut AppRoot, event: &DragMoveEvent<crate::app::BleedDrag>, _window, cx| {
                    this.drag_bleed(event, cx);
                },
            ));
        for step in 0..tokens::BLEED_STEPS {
            let active = step <= index;
            track = track.child(
                div()
                    .debug_selector(move || format!("bleed-step-{step}"))
                    .id(SharedString::from(format!("bleed-step-{step}")))
                    .w(px(9.))
                    .h(px(6.))
                    .rounded(px(3.))
                    .bg(if active { theme.accent } else { theme.track })
                    .cursor_pointer()
                    .on_click(cx.listener(
                        move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.set_bleed_index(step, cx);
                        },
                    )),
            );
        }
        div()
            .flex()
            .items_center()
            .gap(space::SM)
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.muted)
                    .child(language.bleed_padding()),
            )
            .child(track)
            .child(
                div()
                    .w(px(32.))
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.faint)
                    .child(format!("{}%", (index as f32 * 5.0).round() as u32)),
            )
    }

    fn render_keep_second_toggle(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let enabled = self.project.settings.keep_second;
        let language = self.language;
        div()
            .debug_selector(|| "subtitle-keep-second".to_string())
            .id("subtitle-keep-second")
            .flex()
            .items_center()
            .gap(space::XS)
            .h(size::CAPSULE_BUTTON)
            .px(px(4.))
            .rounded(radius::SMALL)
            .cursor_pointer()
            .hover(move |style| style.bg(theme.hover))
            .on_click(
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_keep_second(!enabled, cx);
                }),
            )
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(theme.muted)
                    .child(language.keep_second_line()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .w(px(28.))
                    .h(px(16.))
                    .px(px(2.))
                    .rounded(px(8.))
                    .bg(if enabled { theme.accent } else { theme.track })
                    .when(enabled, |this| this.justify_end())
                    .child(
                        div()
                            .w(px(12.))
                            .h(px(12.))
                            .rounded(px(6.))
                            .bg(theme.surface),
                    ),
            )
    }

    fn render_layout_presets(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.project.settings.layout;
        let language = self.language;
        let mut row = div().flex().items_center().gap(px(2.));
        for (index, target) in LayoutTarget::PRESETS.iter().enumerate() {
            let target = *target;
            let active = target == current;
            row = row.child(
                div()
                    .debug_selector(move || format!("layout-preset-{index}"))
                    .id(SharedString::from(format!("layout-preset-{index}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(radius::PILL)
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::CAPTION)
                    .text_color(if active {
                        theme.accent_text
                    } else {
                        theme.muted
                    })
                    .when(active, |this| this.bg(theme.accent))
                    .hover(move |style| style.bg(theme.hover))
                    .cursor_pointer()
                    .on_click(cx.listener(
                        move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.set_layout_target(target, cx);
                        },
                    ))
                    .child(language.layout_label(target)),
            );
        }
        row
    }

    fn render_toast(&mut self, theme: Theme) -> impl IntoElement {
        let error = self.status.error && !self.detecting;
        let text = if self.detecting {
            let total = self.project.items.len();
            let done = total.saturating_sub(self.pending_detection_count());
            self.language.detecting_progress(done, total)
        } else {
            self.status.text.to_string()
        };
        div()
            .debug_selector(|| "toast".to_string())
            .id("toast")
            .px(px(12.))
            .py(px(6.))
            .rounded(radius::PILL)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .font_family(tokens::FONT_FAMILY)
            .text_size(text_size::CAPTION)
            .text_color(if error { theme.danger } else { theme.muted })
            .child(text)
    }
}

fn action_button(
    id: &'static str,
    glyph: Icon,
    label: &'static str,
    primary: bool,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let mut button = div()
        .debug_selector(move || id.to_string())
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .h(size::CAPSULE_BUTTON)
        .px(px(if label.is_empty() { 8. } else { 11. }))
        .rounded(radius::PILL)
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::BODY)
        .text_color(if primary {
            theme.accent_text
        } else {
            theme.text
        })
        .cursor_pointer();
    if primary {
        button = button
            .bg(theme.accent)
            .hover(move |style| style.opacity(0.9));
    } else {
        button = button.hover(move |style| style.bg(theme.hover));
    }
    button = button.on_click(on_click);
    button
        .child(icon::icon(
            glyph,
            px(16.),
            if primary {
                theme.accent_text
            } else {
                theme.text
            },
        ))
        .when(!label.is_empty(), |this| this.child(label))
}

fn widgets_divider(theme: Theme) -> impl IntoElement {
    div()
        .w(px(1.))
        .h(px(18.))
        .bg(theme.border_subtle)
        .mx(px(2.))
}

struct CapsuleDragGhost;

impl gpui::Render for CapsuleDragGhost {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
