//! 底部单一灵动胶囊：模式、留白或排版预设、添加、撤销、复制、导出。
//!
//! 正常状态是一条窄胶囊。拖动图片时它缩成一个小小的垃圾桶投放目标：安静时是
//! 中性色的垃圾桶加本次会删掉几张，鼠标压上去才张开、变成危险色、写明「松手即可删除」，
//! 松手在别处就取消。

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, Div, DragMoveEvent, IntoElement,
    MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString, div, ease_out_quint, px,
};
use std::time::Duration;

use crate::app::{AppRoot, CapsuleTarget, DragItem, TOAST_EXIT_MS};
use crate::model::{LayoutTarget, Mode};
use crate::ui::hint::HINT_GAP;
use crate::ui::hover::{self, HoverBinding, HoverKey};
use crate::ui::icon::{self, Icon};
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};

/// 提示浮层入场动画时长。
const TOAST_ENTER_MS: u64 = 150;
/// 胶囊换成删除目标（及还原）的形态过渡时长。
const CAPSULE_MORPH_MS: u64 = 140;
/// 操作成功后按钮闪烁的时长。
const SUCCESS_FLASH_MS: u64 = 280;

impl AppRoot {
    pub(crate) fn render_capsule(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dragging = self.dragging.is_some();
        let target = CapsuleTarget::for_state(dragging, self.delete_hover);
        // 形变动画要从上一个形态过渡到当前形态，所以先把上一个形态留下来再覆盖。
        let previous = self.capsule_target_prev;
        if previous.is_some() && previous != Some(target) {
            self.capsule_morph_gen += 1;
        }
        self.capsule_target_prev = Some(target);
        let mut column = div()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(size::CAPSULE_BOTTOM)
            .flex()
            .flex_col()
            .items_center()
            .gap(size::TOAST_GAP);

        if (self.toast_visible || self.detecting || self.toast_closing) && !dragging {
            column = column.child(self.render_toast(theme));
        }

        column.child(self.render_pill(theme, target, previous, cx))
    }

    fn render_pill(
        &mut self,
        theme: Theme,
        target: CapsuleTarget,
        previous: Option<CapsuleTarget>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if target != CapsuleTarget::Idle {
            return self.render_delete_target(theme, target, previous, cx);
        }
        let mut pill = div()
            .flex()
            .items_center()
            .gap(space::SM)
            .h(size::CAPSULE)
            .px(px(10.))
            .rounded(radius::PILL)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_float())
            .occlude()
            // 胶囊盖在画布上方，外部文件落到它上面时根节点收不到放置事件，
            // 这里接一次再转发给同一个处理函数，免得拖到胶囊上就丢图。
            .on_drop(cx.listener(Self::on_file_drop))
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

        let flash = self.success_flash;
        let add_binding = self.hover_binding("capsule-add", cx);
        let add_hint = self
            .render_attached_hint(&HoverKey::widget("capsule-add"), HINT_GAP + size::CAPSULE_BUTTON, theme);
        let add_click =
            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.open_images(cx));
        // 需求 3：添加、复制、导出只留图标；说明文字贴在各自按钮上方（需求 5），
        // 不再统一浮在胶囊居中位置。
        pill = pill.child(widgets_divider(theme)).child(action_button(
            "capsule-add",
            Icon::Plus,
            "",
            theme,
            add_binding,
            add_hint,
            flash,
            add_click,
        ));
        if self.undo.can_undo() {
            let undo_binding = self.hover_binding("capsule-undo", cx);
            let undo_hint = self.render_attached_hint(
                &HoverKey::widget("capsule-undo"),
                HINT_GAP + size::CAPSULE_BUTTON,
                theme,
            );
            let undo_click =
                cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.undo_edit(cx));
            pill = pill.child(action_button(
                "capsule-undo",
                Icon::Undo,
                "",
                theme,
                undo_binding,
                undo_hint,
                flash,
                undo_click,
            ));
        }
        let copy_binding = self.hover_binding("capsule-copy", cx);
        let copy_hint = self.render_attached_hint(
            &HoverKey::widget("capsule-copy"),
            HINT_GAP + size::CAPSULE_BUTTON,
            theme,
        );
        let copy_click =
            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.copy_image(cx));
        let export_binding = self.hover_binding("export-png", cx);
        let export_hint = self.render_attached_hint(
            &HoverKey::widget("export-png"),
            HINT_GAP + size::CAPSULE_BUTTON,
            theme,
        );
        let export_click =
            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| this.export_png(cx));
        // 复制和导出同属“把成品拿走”的操作，与前面的编辑类按钮之间用分割线隔开；
        // 两个按钮地位相等，都不用强调色。
        pill = pill
            .child(widgets_divider(theme))
            .child(action_button(
                "capsule-copy",
                Icon::Copy,
                "",
                theme,
                copy_binding,
                copy_hint,
                flash,
                copy_click,
            ))
            .child(action_button(
                "export-png",
                Icon::Export,
                "",
                theme,
                export_binding,
                export_hint,
                flash,
                export_click,
            ));
        self.animate_capsule_morph(pill, previous, target, theme)
    }

    /// 拖动图片时的投放目标。
    ///
    /// 安静态是一个中性色的小药丸：垃圾桶加本次会删掉几张。鼠标压上去才张开、
    /// 换成危险色并写明「松手即可删除」，和手机上拖到垃圾桶附近才变红是同一个体感。
    fn render_delete_target(
        &mut self,
        theme: Theme,
        target: CapsuleTarget,
        previous: Option<CapsuleTarget>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let armed = target == CapsuleTarget::Armed;
        let count = self.selected_count().max(1);
        let color = if armed {
            theme.accent_text
        } else {
            theme.muted
        };
        let mut zone = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(space::XS)
            .h(size::DELETE_TARGET_H)
            .px(size::DELETE_TARGET_PAD)
            .rounded(radius::PILL)
            .bg(if armed { theme.danger } else { theme.surface })
            .border_1()
            .border_color(if armed {
                theme.danger
            } else {
                theme.border_subtle
            })
            .shadow(theme.shadow_float())
            .occlude()
            .on_drop(cx.listener(Self::on_file_drop))
            .debug_selector(|| "delete-zone".to_string())
            .cursor(gpui::CursorStyle::PointingHand)
            .on_drag_move::<DragItem>(cx.listener(
                |this: &mut AppRoot, event: &DragMoveEvent<DragItem>, _window, cx| {
                    // 元素张开会变高变宽，用指针和当前 bounds 直接比，天然支持
                    // 「压上去张开、移开收起」，不依赖画布那边的鼠标移动事件。
                    let pointer = event.event.position;
                    let hovered = event.bounds.contains(&pointer);
                    this.set_delete_hover(hovered, cx);
                    // 指针压在窗口底部的胶囊上时，画布收不到拖动移动事件。
                    // 这里补记位置，跟手小图和贴边自动滚动才不会停在上一次。
                    this.drag_pointer = Some(pointer);
                    this.refresh_auto_scroll(cx);
                    cx.notify();
                },
            ))
            .on_drop(
                cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                    this.drop_on_delete(dragged.id, cx);
                }),
            )
            .child(icon::icon(Icon::Trash, size::DELETE_TARGET_ICON, color));
        if armed {
            zone = zone.child(
                div()
                    .debug_selector(|| "delete-target-armed".to_string())
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::BODY)
                    .font_weight(tokens::weight::STRONG)
                    .text_color(color)
                    .child(self.language.delete_zone_hover().to_string()),
            );
        } else {
            // 安静态只报一个数字：这次会删掉几张。
            zone = zone.child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::BODY)
                    .font_weight(tokens::weight::STRONG)
                    .text_color(color)
                    .child(count.to_string()),
            );
        }
        self.animate_capsule_morph(zone, previous, target, theme)
    }

    fn animate_capsule_morph(
        &self,
        pill: Div,
        previous: Option<CapsuleTarget>,
        target: CapsuleTarget,
        theme: Theme,
    ) -> AnyElement {
        let pill = if self.capsule_morph_gen == 0 {
            pill.into_any_element()
        } else {
            // 三态之间过渡：正常胶囊 / 安静的垃圾桶 / 张开变红的垃圾桶。
            let idle_h = f32::from(size::CAPSULE);
            let idle_pad = 10.0;
            let idle_bg = theme.surface;
            let idle_bd = theme.border_subtle;
            let dragging_h = f32::from(size::DELETE_TARGET_H);
            let dragging_pad = f32::from(size::DELETE_TARGET_PAD);
            let armed_h = f32::from(size::DELETE_TARGET_ARMED_H);
            let armed_pad = f32::from(size::DELETE_TARGET_ARMED_PAD);
            let (h0, pad0, bg0, bd0) = match previous {
                Some(CapsuleTarget::Armed) => (armed_h, armed_pad, theme.danger, theme.danger),
                Some(CapsuleTarget::Dragging) => {
                    (dragging_h, dragging_pad, theme.surface, theme.border_subtle)
                }
                Some(CapsuleTarget::Idle) => (idle_h, idle_pad, idle_bg, idle_bd),
                None => (idle_h, idle_pad, idle_bg, idle_bd),
            };
            let (h1, pad1, bg1, bd1) = match target {
                CapsuleTarget::Idle => (idle_h, idle_pad, idle_bg, idle_bd),
                CapsuleTarget::Dragging => {
                    (dragging_h, dragging_pad, theme.surface, theme.border_subtle)
                }
                CapsuleTarget::Armed => (armed_h, armed_pad, theme.danger, theme.danger),
            };
            pill.with_animation(
                SharedString::from(format!("capsule-morph-{}", self.capsule_morph_gen)),
                Animation::new(Duration::from_millis(CAPSULE_MORPH_MS))
                    .with_easing(ease_out_quint()),
                move |el, delta| {
                    el.h(px(h0 + (h1 - h0) * delta))
                        .px(px(pad0 + (pad1 - pad0) * delta))
                        .bg(crate::ui::theme::lerp_hsla(bg0, bg1, delta))
                        .border_color(crate::ui::theme::lerp_hsla(bd0, bd1, delta))
                },
            )
            .into_any_element()
        };
        div()
            .debug_selector(|| "capsule".to_string())
            .id("capsule")
            .child(pill)
            .into_any_element()
    }

    fn mode_button(
        &mut self,
        mode: Mode,
        current: Mode,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = mode == current;
        let label = match mode {
            Mode::Subtitle => self.language.mode_label(Mode::Subtitle),
            Mode::Layout => self.language.mode_label(Mode::Layout),
        };
        let key = match mode {
            Mode::Subtitle => "mode-subtitle",
            Mode::Layout => "mode-layout",
        };
        let fade = self.hover_gate.fade_delta(&HoverKey::widget(key));
        let button = div()
            .debug_selector(move || key.to_string())
            .id(key)
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
            .bg(hover::fade_background(
                active.then_some(theme.surface_elevated),
                theme.hover,
                fade,
            ))
            .on_hover(
                cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget(key), *hovered, cx);
                }),
            )
            .on_click(
                cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                    this.set_mode(mode, cx);
                }),
            )
            .child(label);
        button.into_any_element()
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

    fn render_keep_second_toggle(&mut self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.project.settings.keep_second;
        let language = self.language;
        let fade = self
            .hover_gate
            .fade_delta(&HoverKey::widget("subtitle-keep-second"));
        let toggle = div()
            .debug_selector(|| "subtitle-keep-second".to_string())
            .id("subtitle-keep-second")
            .flex()
            .items_center()
            .gap(space::XS)
            .h(size::CAPSULE_BUTTON)
            .px(px(4.))
            .rounded(radius::SMALL)
            .cursor_pointer()
            .bg(hover::fade_background(None, theme.hover, fade))
            .on_hover(
                cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("subtitle-keep-second"), *hovered, cx);
                }),
            )
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
            );
        toggle.into_any_element()
    }

    fn render_layout_presets(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.project.settings.layout;
        let language = self.language;
        let mut row = div().flex().items_center().gap(px(2.));
        for (index, target) in LayoutTarget::PRESETS.iter().enumerate() {
            let target = *target;
            let active = target == current;
            let element_id = SharedString::from(format!("layout-preset-{index}"));
            // 需求 3：横向、竖向两个预设换成方向图标，文字只在悬停提示里出现；
            // 比例预设（16:9 这类）仍然写文字。悬停用语义化的 key 上报，
            // 这样提示文字不用去猜第几个预设才是横排。
            let hover_key = match target {
                LayoutTarget::Row => HoverKey::widget("layout-row"),
                LayoutTarget::Column => HoverKey::widget("layout-column"),
                _ => HoverKey::Widget(element_id.clone()),
            };
            let fade = self.hover_gate.fade_delta(&hover_key);
            let preset = div()
                .debug_selector({
                    let label_key = element_id.clone();
                    move || label_key.to_string()
                })
                .id(element_id.clone())
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
                .bg(hover::fade_background(
                    active.then_some(theme.accent),
                    theme.hover,
                    fade,
                ))
                .cursor_pointer()
                .on_hover(cx.listener({
                    let key = hover_key.clone();
                    move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                        this.set_hover(key.clone(), *hovered, cx);
                    }
                }))
                .on_click(
                    cx.listener(move |this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                        this.set_layout_target(target, cx);
                    }),
                );
            let preset = match target {
                LayoutTarget::Row => preset.child(icon::icon(Icon::ArrangeRow, px(14.), preset_color(active, theme))),
                LayoutTarget::Column => preset.child(icon::icon(
                    Icon::ArrangeColumn,
                    px(14.),
                    preset_color(active, theme),
                )),
                _ => preset.child(language.layout_label(target)),
            };
            // 需求 5：横排、竖排这两个纯图标预设的说明贴在各自按钮上方，
            // 比例预设没有说明，也就不多包一层。
            let mut cell = div().relative().child(preset);
            if let Some(hint) = self.render_attached_hint(&hover_key, HINT_GAP + px(26.), theme) {
                cell = cell.child(hint);
            }
            row = row.child(cell);
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
        let toast = div()
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
            .child(text);
        if self.toast_closing {
            toast
                .with_animation(
                    SharedString::from(format!("toast-exit-{}", self.toast_token)),
                    Animation::new(Duration::from_millis(TOAST_EXIT_MS))
                        .with_easing(ease_out_quint()),
                    |el, delta| el.opacity(1.0 - delta),
                )
                .into_any_element()
        } else {
            toast
                .with_animation(
                    "toast-enter",
                    Animation::new(Duration::from_millis(TOAST_ENTER_MS))
                        .with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).top(px(6.0 * (1.0 - delta))),
                )
                .into_any_element()
        }
    }
}

/// 胶囊里的通用操作按钮。悬停即时生效，反馈直接显示。
///
/// `hint` 是这个按钮的悬停说明气泡（需求 5）：有它时按钮外面包一层
/// `relative()`，气泡就贴在按钮正上方，而不是统一浮在胶囊中间。
fn action_button(
    id: &'static str,
    glyph: Icon,
    label: &'static str,
    theme: Theme,
    hover: HoverBinding,
    hint: Option<AnyElement>,
    flash: Option<(&'static str, u64)>,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
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
        .text_color(theme.text)
        .cursor_pointer()
        .bg(hover::fade_background(None, theme.hover, hover.fade))
        .on_hover(hover.on_event)
        .on_click(on_click);
    if let Some((_key, generation)) = flash.filter(|(key, _)| *key == id) {
        let overlay = div()
            .debug_selector(move || format!("{id}-flash"))
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .rounded(radius::PILL)
            .with_animation(
                SharedString::from(format!("{id}-flash-{generation}")),
                Animation::new(Duration::from_millis(SUCCESS_FLASH_MS))
                    .with_easing(ease_out_quint()),
                move |el, delta| el.bg(theme.accent.opacity(0.30 * (1.0 - delta))),
            );
        button = button.child(overlay);
    }
    let button = button
        .child(icon::icon(glyph, px(16.), theme.text))
        .when(!label.is_empty(), |this| this.child(label));
    // 气泡绝对定位，必须以按钮为基准摆放，所以外面包一层 relative。
    let mut wrapper = div().relative().child(button);
    if let Some(hint) = hint {
        wrapper = wrapper.child(hint);
    }
    wrapper.into_any_element()
}

/// 排版预设图标的颜色：选中的用强调色上的文字色，未选中的用次要文字色。
fn preset_color(active: bool, theme: Theme) -> gpui::Hsla {
    if active { theme.accent_text } else { theme.muted }
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
