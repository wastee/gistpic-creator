//! 放大浮层里的两种视图：可编辑的边界拖动视图，和只读的叠放摊开视图。

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, CursorStyle, DragMoveEvent, IntoElement, MouseButton, MouseUpEvent,
    ObjectFit, SharedString, StyledImage, div, img, px,
};

use crate::app::{AppRoot, ManualDrag, ManualEdge};
use crate::model::ManualBand;
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, size, space};
use crate::ui::widgets::{self, DragGhost};

impl AppRoot {
    /// 可编辑视图：图片叠放显示 + 上下边界线，拖线改裁剪范围。
    pub(crate) fn zoom_layer_editable(
        &mut self,
        theme: Theme,
        ids: &[crate::model::ItemId],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let images = self.zoom_layer_images(ids);
        let primary = images
            .first()
            .map(|image| (image.id, image.width, image.height));
        let Some((primary_id, width, height)) = primary else {
            return div()
                .flex_1()
                .child(widgets::hint_text(self.language.selection_hint(), theme));
        };

        // 边界比例以最上面那张图为准。
        let band = self
            .current_manual_for(primary_id)
            .unwrap_or(ManualBand {
                top: 0.0,
                bottom: 1.0,
            });
        let top_y = (height * band.top).clamp(0.0, height);
        let bottom_y = (height * band.bottom).clamp(0.0, height);
        let band_height = (bottom_y - top_y).max(0.0);
        let top_dim = top_y;
        let bottom_dim = (height - bottom_y).max(0.0);
        let ghost_theme = theme;
        let drag_label: SharedString = self.language.manual_crop().into();
        let top_label = drag_label.clone();
        let bottom_label = drag_label.clone();
        let editing = self.crop_edit == Some(primary_id);
        let hidden_layers = images
            .len()
            .saturating_sub(tokens::STACK_MAX_LAYERS);

        // 底下几层先画，最上面一层最后画（在同一个相对容器里靠顺序叠）。
        // 层数太多会叠成一片看不清，只画最上面几层，剩下的用一行文字说明。
        let mut layers = div()
            .relative()
            .w(px(width))
            .h(px(height))
            .flex()
            .flex_shrink_0();
        for (depth, image) in images
            .iter()
            .take(tokens::STACK_MAX_LAYERS)
            .enumerate()
            .rev()
        {
            let (offset, scale, opacity) = tokens::stack_layer(depth);
            let layer_w = (width * scale).max(1.0);
            let layer_h = (height * scale).max(1.0);
            let mut layer = div()
                .absolute()
                .left(px(offset))
                .top(px(-offset))
                .w(px(layer_w))
                .h(px(layer_h))
                .rounded(radius::SMALL)
                .overflow_hidden();
            if depth > 0 {
                layer = layer.opacity(opacity);
            }
            layer = layer.child(
                img(image.image.clone())
                    .w(px(layer_w))
                    .h(px(layer_h))
                    .object_fit(ObjectFit::Fill),
            );
            layers = layers.child(layer);
        }

        // 多选时在底部加一行说明，说明改动会同步到哪些图。
        // 这段先算好：它的文字要读 self.language，和下面借用 self 的 stage 不能交错。
        let multi_hint = if ids.len() > 1 {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .child(widgets::faint_text(
                        self.language.zoom_layer_sync_hint(ids.len()),
                        theme,
                    ))
                    .child(widgets::faint_text(
                        self.language.zoom_layer_sync_note(),
                        theme,
                    ))
                    .when(hidden_layers > 0, |this| {
                        this.child(widgets::faint_text(
                            self.language.stack_hidden_layers(hidden_layers),
                            theme,
                        ))
                    }),
            )
        } else {
            None
        };

        let stage = self.zoom_edit_stage(
            theme,
            primary_id,
            width,
            height,
            editing,
            (top_y, bottom_y, band_height, top_dim, bottom_dim),
            (top_label, bottom_label, ghost_theme),
            cx,
        );

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::MD)
            .p(space::XL)
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .h(px(height))
                    .child(layers)
                    .child(stage),
            )
            .children(multi_hint)
    }

    /// 可编辑视图里盖在最上面那层图上的东西：选区外的遮罩和可拖的两条边界。
    #[allow(clippy::too_many_arguments)]
    fn zoom_edit_stage(
        &mut self,
        theme: Theme,
        primary_id: crate::model::ItemId,
        width: f32,
        height: f32,
        editing: bool,
        dims: (f32, f32, f32, f32, f32),
        labels: (SharedString, SharedString, Theme),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (top_y, bottom_y, band_height, top_dim, bottom_dim) = dims;
        let (top_label, bottom_label, ghost_theme) = labels;

        let mut overlay = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(width))
            .h(px(height))
            .on_drag_move::<ManualDrag>(cx.listener(
                move |this: &mut AppRoot,
                      event: &DragMoveEvent<ManualDrag>,
                      _window,
                      cx| {
                    let edge = event.drag(cx).edge;
                    let ratio = (f32::from(event.event.position.y) - f32::from(event.bounds.top()))
                        / f32::from(event.bounds.size.height);
                    this.update_manual_drag(primary_id, edge, ratio, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &MouseUpEvent, _window, _cx| {
                    this.end_manual_drag();
                }),
            );

        if !editing {
            return overlay;
        }

        overlay = overlay
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h(px(top_dim))
                    .bg(theme.dim()),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h(px(bottom_dim))
                    .bg(theme.dim()),
            )
            .child(
                div()
                    .absolute()
                    .top(px(top_y))
                    .left(px(0.))
                    .w_full()
                    .h(px(band_height))
                    .border_2()
                    .border_color(theme.text),
            )
            .child(
                div()
                    .debug_selector(|| format!("layer-top-{primary_id}"))
                    .id(SharedString::from(format!("layer-top-{primary_id}")))
                    .absolute()
                    .top(px(top_y))
                    .left(px(0.))
                    .w_full()
                    .h(size::BAND_HIT)
                    .cursor(CursorStyle::ResizeUpDown)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(
                            move |this: &mut AppRoot,
                                  _: &gpui::MouseDownEvent,
                                  _window,
                                  cx| {
                                this.begin_manual_drag(primary_id, ManualEdge::Top, cx);
                            },
                        ),
                    )
                    .on_click(cx.listener(
                        |_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                        },
                    ))
                    .on_drag(
                        ManualDrag {
                            id: primary_id,
                            edge: ManualEdge::Top,
                        },
                        move |_value, _offset, _window, cx| {
                            cx.new(|_| DragGhost {
                                label: top_label.clone(),
                                theme: ghost_theme,
                            })
                        },
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(size::BAND_LINE)
                            .bg(theme.text),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| format!("layer-bottom-{primary_id}"))
                    .id(SharedString::from(format!("layer-bottom-{primary_id}")))
                    .absolute()
                    .top(px((bottom_y - f32::from(size::BAND_HIT)).max(0.0)))
                    .left(px(0.))
                    .w_full()
                    .h(size::BAND_HIT)
                    .cursor(CursorStyle::ResizeUpDown)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(
                            move |this: &mut AppRoot,
                                  _: &gpui::MouseDownEvent,
                                  _window,
                                  cx| {
                                this.begin_manual_drag(primary_id, ManualEdge::Bottom, cx);
                            },
                        ),
                    )
                    .on_click(cx.listener(
                        |_this: &mut AppRoot, _: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                        },
                    ))
                    .on_drag(
                        ManualDrag {
                            id: primary_id,
                            edge: ManualEdge::Bottom,
                        },
                        move |_value, _offset, _window, cx| {
                            cx.new(|_| DragGhost {
                                label: bottom_label.clone(),
                                theme: ghost_theme,
                            })
                        },
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(size::BAND_LINE)
                            .bg(theme.text),
                    ),
            );

        overlay
    }

    /// 纯展示的叠放摊开视图：每层进一步错开，只用来整体看一眼。
    pub(crate) fn zoom_layer_stack_view(
        &mut self,
        theme: Theme,
        ids: &[crate::model::ItemId],
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let images = self.zoom_layer_images(ids);
        let base = images
            .first()
            .map(|image| image.width.min(520.0))
            .unwrap_or(520.0);

        let mut canvas = div().relative().w(px(600.)).h(px(360.));
        let count = images.len();
        for (depth, image) in images
            .iter()
            .take(tokens::STACK_MAX_LAYERS)
            .enumerate()
            .rev()
        {
            let (offset, scale, opacity) = tokens::stack_layer(depth);
            // 逐层缩小，并且保持每张图自己的宽高比。
            let layer_w = (base * 0.8 * scale).max(1.0);
            let aspect = image.height / image.width.max(1.0);
            let layer_h = (layer_w * aspect).clamp(1.0, 320.0);
            let mut layer = div()
                .absolute()
                .left(px(20.0 + offset))
                .top(px(20.0 + offset * 0.5))
                .w(px(layer_w))
                .h(px(layer_h))
                .rounded(radius::SMALL)
                .overflow_hidden()
                .opacity(if depth == 0 { 1.0 } else { opacity });
            layer = layer.child(
                img(image.image.clone())
                    .w(px(layer_w))
                    .h(px(layer_h))
                    .object_fit(ObjectFit::Contain),
            );
            canvas = canvas.child(layer);
        }

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::MD)
            .p(space::XL)
            .child(canvas)
            .child(widgets::faint_text(
                self.language.stack_preview_hint(count),
                theme,
            ))
    }
}
