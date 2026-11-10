//! 放大浮层：把一张图放大到能对准边界，多选时按错位叠放同步调整。

use std::sync::Arc;

use image::RgbImage;

use gpui::prelude::*;
use gpui::{ClickEvent, Context, IntoElement, div, px};

use crate::app::{AppRoot, PREVIEW_TARGET_WIDTH};
use crate::imaging::crop::CropBand;
use crate::model::ItemId;
use crate::ui::icon::Icon;
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space, text_size};
use crate::ui::widgets;

/// 浮层里一张图要显示的信息。
pub(crate) struct LayerImage {
    pub(crate) id: ItemId,
    pub(crate) image: Arc<gpui::RenderImage>,
    /// 显示宽高，已经按面积算好。
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl AppRoot {
    /// 浮层里要一起调整的图片 id 列表，最上面那张排在前面。
    fn zoom_layer_ids(&self) -> Vec<ItemId> {
        let mut ids: Vec<ItemId> = self
            .selected_ids
            .iter()
            .copied()
            .filter(|id| {
                self.project.items.iter().any(|item| {
                    item.id == *id && !matches!(item.status, crate::model::ItemStatus::Pending)
                })
            })
            .collect();
        // 当前点击的那张排第一，其余的按列表顺序。
        if let Some(active) = self.selected
            && let Some(position) = ids.iter().position(|id| *id == active)
        {
            let active = ids.remove(position);
            ids.insert(0, active);
        }
        ids
    }

    pub(crate) fn render_zoom_layer(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ids = self.zoom_layer_ids();
        let count = ids.len();

        let header = div()
            .h(size::LAYER_HEADER)
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px(space::LG)
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY)
                    .text_size(text_size::TITLE)
                    .font_weight(tokens::weight::STRONG)
                    .text_color(theme.text)
                    .child(self.language.zoom_layer_title(count)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(count > 1, |this| {
                        this.child(widgets::text_button(
                            "layer-stack-preview",
                            self.language.stack_preview(),
                            theme,
                            true,
                            cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                                this.toggle_zoom_stack_preview(cx);
                            }),
                        ))
                    })
                    .child(widgets::icon_button(
                        "layer-close",
                        Icon::Close,
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.close_zoom_layer(cx);
                        }),
                    )),
            );

        // 三个部分都要借用 self 和 cx，所以先各自算完转成 AnyElement，再拼起来。
        let header = header.into_any_element();
        let footer = self.zoom_layer_footer(theme, cx).into_any_element();
        let body = if self.zoom_stack_preview && count > 1 {
            self.zoom_layer_stack_view(theme, &ids, cx).into_any_element()
        } else {
            self.zoom_layer_editable(theme, &ids, cx).into_any_element()
        };

        div()
            .id("zoom-layer")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            // 底下一层遮罩，点它关掉浮层
            .child(
                div()
                    .id("zoom-layer-scrim")
                    .absolute()
                    .inset_0()
                    .bg(theme.scrim())
                    .on_click(cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                        this.close_zoom_layer(cx);
                    })),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .m(space::XL)
                            .rounded(radius::LARGE)
                            .bg(theme.surface)
                            .overflow_hidden()
                            .child(header)
                            .child(body)
                            .child(footer),
                    ),
            )
    }

    fn zoom_layer_footer(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.selected;
        let band = active.and_then(|id| self.current_manual_for(id));
        let position_text = band
            .map(|band| {
                self.language.manual_position(
                    (band.top * 100.0).round() as u32,
                    (band.bottom * 100.0).round() as u32,
                )
            })
            .unwrap_or_default();

        let can_reset = active.is_some_and(|id| {
            self.project
                .items
                .iter()
                .any(|item| item.id == id && item.manual.is_some())
        });
        // 「应用到全部」只在已经有手动框选、并且一共有不止一张图时才有意义。
        let can_apply_all = can_reset && self.project.items.len() > 1;

        div()
            .h(size::LAYER_FOOTER)
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px(space::LG)
            .child(widgets::faint_text(position_text, theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(widgets::text_button(
                        "layer-reset",
                        self.language.reset_to_auto(),
                        theme,
                        can_reset,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.clear_manual_band(cx);
                        }),
                    ))
                    .child(widgets::text_button(
                        "layer-apply-all",
                        self.language.apply_to_all(),
                        theme,
                        can_apply_all,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.apply_manual_to_all(cx);
                        }),
                    ))
                    .child(widgets::text_button(
                        "layer-done",
                        self.language.manual_done(),
                        theme,
                        true,
                        cx.listener(|this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            this.close_zoom_layer(cx);
                        }),
                    )),
            )
    }

    /// 按浮层可用区域算出每张图要显示的尺寸。
    pub(crate) fn zoom_layer_images(&mut self, ids: &[ItemId]) -> Vec<LayerImage> {
        // 浮层可用区域按窗口最小尺寸保守估算：窗口 1240 宽、720 高，
        // 减掉边距、头尾和两侧留白。
        let available_w = 1240.0 - f32::from(space::XL) * 4.0 - 120.0;
        let available_h = 720.0
            - f32::from(size::LAYER_HEADER)
            - f32::from(size::LAYER_FOOTER)
            - f32::from(space::XL) * 4.0;

        // 先把要用的数据抄出来，避免一边借用 items 一边调 &mut self 的方法。
        let mut specs: Vec<(ItemId, Arc<RgbImage>, u32, u32)> = Vec::new();
        for id in ids {
            let Some(item) = self.project.items.iter().find(|item| item.id == *id) else {
                continue;
            };
            specs.push((*id, item.rgb.clone(), item.width, item.height));
        }

        let mut images = Vec::new();
        for (id, rgb, width, height) in specs {
            let source_height = height.max(1) as f32;
            let band = CropBand {
                y0: 0,
                y1: height.max(1),
            };
            let image = self.preview_for(id, rgb.as_ref(), band, PREVIEW_TARGET_WIDTH);
            let scale = (available_w / width.max(1) as f32)
                .min(available_h / source_height)
                .min(1.6);
            images.push(LayerImage {
                id,
                image,
                width: (width as f32 * scale).max(1.0),
                height: (source_height * scale).max(1.0),
            });
        }
        images
    }
}
