//! 需求 15：画布左下角的全局预览导航器。
//!
//! 作品放大到一屏放不下时，右下角的缩放胶囊只能告诉你「现在多大」，没法告诉你
//! 「现在看到的是哪一块」，也没法直接跳过去。这里补一个缩略图加一个视口框：
//! 拖框就平移画布，缩放和作品数据都不动。
//!
//! 两条设计约束：
//!
//! 1. 不生成全尺寸合成图，也不用后台渲染。缩略图直接用已有的每一张图的预览
//!    位图（`preview_for` 的缓存）按 `CanvasLayout` 的图块矩形缩放摆放，用的
//!    是界面本来就要画的那些位图。
//! 2. 作品没超出可视区时不显示。没东西可平移的时候挂一个框在那里只会挡画面。
//!
//! 摆放规则和「拖动框怎么换算成滚动偏移」都是纯函数，见本文件下方的测试。

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, IntoElement, MouseButton, MouseDownEvent,
    MouseUpEvent, ObjectFit, Pixels, Point, Size, Window, div, img, px,
};

use crate::app::{AppRoot, NavigatorDrag};
use crate::ui::canvas::Rect;
use crate::ui::hover::HoverKey;
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius};

/// 导航器里那张缩略图占的矩形（导航器自己的坐标系，原点在导航器左上角）。
///
/// 缩略图按等比缩放塞进导航器内框并居中，长宽比和作品一致。
pub fn thumbnail_rect(artwork: Size<f32>, frame: Size<f32>) -> Rect {
    if artwork.width <= 0.0 || artwork.height <= 0.0 || frame.width <= 0.0 || frame.height <= 0.0 {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let scale = (frame.width / artwork.width).min(frame.height / artwork.height);
    let w = artwork.width * scale;
    let h = artwork.height * scale;
    Rect::new((frame.width - w) / 2.0, (frame.height - h) / 2.0, w, h)
}

/// 视口框在缩略图里的矩形。
///
/// `scroll` 是画布当前的滚动偏移（向右、向下为负），`max_scroll` 是可滚动范围
/// （正值）。缩放比例是「缩略图边长 / 作品对应边长」，所以可视区在缩略图里占
/// `viewport / 缩放比例` 那么大。
///
/// 框比缩略图还大时按缩略图裁掉：作品没溢出时框就是整张缩略图。
pub fn viewport_frame(thumb: Rect, artwork: Size<f32>, viewport: Size<f32>, scroll: Point<f32>) -> Rect {
    if thumb.w <= 0.0 || thumb.h <= 0.0 || artwork.width <= 0.0 || artwork.height <= 0.0 {
        return thumb;
    }
    let scale_x = thumb.w / artwork.width;
    let scale_y = thumb.h / artwork.height;
    let w = (viewport.width * scale_x).min(thumb.w);
    let h = (viewport.height * scale_y).min(thumb.h);
    // 滚动偏移是负的，取反之后就是「已经看过多少」，再乘缩放比例。
    let x = (thumb.x - scroll.x * scale_x).clamp(thumb.x, (thumb.right() - w).max(thumb.x));
    let y = (thumb.y - scroll.y * scale_y).clamp(thumb.y, (thumb.bottom() - h).max(thumb.y));
    Rect::new(x, y, w, h)
}

/// 把视口框在缩略图里拖到 `pointer` 处，换算成画布要用的滚动偏移。
///
/// `grab` 是按下时指针相对框左上角的偏移：拖动全程保持这个偏移，框才不会在
/// 指针底下跳一下。返回值夹在 `0..=max_scroll`（正值），调用方取负号交给滚动区。
pub fn scroll_for_frame(pointer: Point<f32>, grab: Point<f32>, thumb: Rect, artwork: Size<f32>, viewport: Size<f32>, max_scroll: (f32, f32)) -> Point<f32> {
    if thumb.w <= 0.0 || thumb.h <= 0.0 || artwork.width <= 0.0 || artwork.height <= 0.0 {
        return Point::new(0.0, 0.0);
    }
    let scale_x = thumb.w / artwork.width;
    let scale_y = thumb.h / artwork.height;
    // 框左上角在缩略图里的位置，换算回作品里的位置，就是已经滚过的距离。
    let frame_x = (pointer.x - grab.x).clamp(thumb.x, thumb.right());
    let frame_y = (pointer.y - grab.y).clamp(thumb.y, thumb.bottom());
    let scrolled_x = (frame_x - thumb.x) / scale_x;
    let scrolled_y = (frame_y - thumb.y) / scale_y;
    let _ = viewport;
    Point::new(
        scrolled_x.clamp(0.0, max_scroll.0.max(0.0)),
        scrolled_y.clamp(0.0, max_scroll.1.max(0.0)),
    )
}

/// 导航器面板的描边宽度。渲染时用的是 `.border_1()`，两处必须一致，
/// 否则缩略图的内框尺寸会和 `thumbnail_rect` 算出来的差 1 像素，居中就会有偏差。
const PANEL_BORDER: f32 = 1.0;

/// 面板里真正能放缩略图的内框尺寸：扣掉描边和内边距。
fn thumbnail_area(nav: Rect) -> Size<f32> {
    let inset = 2.0 * (f32::from(tokens::NAVIGATOR_PAD) + PANEL_BORDER);
    Size {
        width: (nav.w - inset).max(0.0),
        height: (nav.h - inset).max(0.0),
    }
}

/// 缩略图在窗口里的绝对矩形：按作品长宽比缩放、放进导航器内框并居中。
///
/// 渲染和鼠标换算都从这一个结果取，避免两边各算一遍、算出来的位置对不上。
pub fn thumbnail_bounds(nav: Rect, artwork: Size<f32>) -> Rect {
    let local = thumbnail_rect(artwork, thumbnail_area(nav));
    let inset = f32::from(tokens::NAVIGATOR_PAD) + PANEL_BORDER;
    Rect::new(
        nav.x + inset + local.x,
        nav.y + inset + local.y,
        local.w,
        local.h,
    )
}

/// 导航器的矩形：画布左下角，避开缩放胶囊和底部留白。
///
/// `viewport` 是画布可视区域。返回 `None` 表示作品没有超出可视区，不该显示。
pub fn navigator_rect(viewport: Size<Pixels>, artwork: Size<f32>, max_scroll: (f32, f32)) -> Option<Rect> {
    // 两个方向都没溢出就没什么可平移的，不显示。
    if max_scroll.0 <= 0.5 && max_scroll.1 <= 0.5 {
        return None;
    }
    if artwork.width <= 0.0 || artwork.height <= 0.0 {
        return None;
    }
    let viewport_w = f32::from(viewport.width);
    let viewport_h = f32::from(viewport.height);
    let w = f32::from(tokens::NAVIGATOR_W);
    let h = f32::from(tokens::NAVIGATOR_H);
    // 竖向：贴在画布左下角。底部留出右下角缩放胶囊的位置，免得两块叠在一起。
    let bottom_reserved = f32::from(tokens::NAVIGATOR_BOTTOM);
    let x = f32::from(tokens::ZOOM_CHIP_GAP);
    let y = (viewport_h - bottom_reserved - h).max(0.0);
    // 窗口小到放不下时不显示，硬塞会盖住画布。
    if viewport_w < w + 2.0 * f32::from(tokens::ZOOM_CHIP_GAP) || y + h > viewport_h {
        return None;
    }
    Some(Rect::new(x, y, w, h))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn artwork() -> Size<f32> {
        Size {
            width: 1600.0,
            height: 2400.0,
        }
    }

    fn viewport() -> Size<f32> {
        Size {
            width: 800.0,
            height: 600.0,
        }
    }

    fn thumb() -> Rect {
        thumbnail_rect(
            artwork(),
            Size {
                width: 160.0,
                height: 120.0,
            },
        )
    }

    /// 缩略图等比缩放并居中，比例和作品一致。
    #[test]
    fn the_thumbnail_keeps_the_artwork_ratio() {
        let t = thumb();
        let artwork_ratio = artwork().width / artwork().height;
        let thumb_ratio = t.w / t.h;
        assert!(
            (artwork_ratio - thumb_ratio).abs() < 1e-3,
            "缩略图比例应与作品一致：作品 {artwork_ratio:.4}，缩略图 {thumb_ratio:.4}"
        );
        assert!(t.w <= 160.0 + 1e-3 && t.h <= 120.0 + 1e-3, "缩略图不该超出内框：{t:?}");
        assert!(t.x >= 0.0 && t.y >= 0.0, "缩略图应在内框里居中：{t:?}");
    }

    /// 没滚动时视口框贴住缩略图左上角。
    #[test]
    fn the_frame_sits_at_the_top_when_nothing_is_scrolled() {
        let t = thumb();
        let frame = viewport_frame(
            t,
            artwork(),
            viewport(),
            Point::new(0.0, 0.0),
        );
        assert!((frame.x - t.x).abs() < 1e-3, "没滚动时框应贴左上角：{frame:?}");
        assert!((frame.y - t.y).abs() < 1e-3, "没滚动时框应贴左上角：{frame:?}");
        assert!(frame.w < t.w, "可视区比作品小，框应比缩略图小：{frame:?}");
    }

    /// 滚到底时视口框贴住缩略图右下角，说明位置和滚动量对得上。
    #[test]
    fn the_frame_reaches_the_bottom_right_when_fully_scrolled() {
        let t = thumb();
        let max = (artwork().width - viewport().width, artwork().height - viewport().height);
        let frame = viewport_frame(
            t,
            artwork(),
            viewport(),
            Point::new(-max.0, -max.1),
        );
        assert!(
            (frame.right() - t.right()).abs() < 1e-3,
            "滚到底时框应贴右边：框右 {:.2}，缩略图右 {:.2}",
            frame.right(),
            t.right()
        );
        assert!(
            (frame.bottom() - t.bottom()).abs() < 1e-3,
            "滚到底时框应贴下边：框下 {:.2}，缩略图下 {:.2}",
            frame.bottom(),
            t.bottom()
        );
    }

    /// 视口框永远不会超出缩略图。
    #[test]
    fn the_frame_never_escapes_the_thumbnail() {
        let t = thumb();
        for scroll in [0.0_f32, -100.0, -5000.0, 500.0] {
            for other in [0.0_f32, -200.0, -9000.0] {
                let frame = viewport_frame(
                    t,
                    artwork(),
                    viewport(),
                    Point::new(scroll, other),
                );
                assert!(
                    frame.x >= t.x - 1e-3 && frame.right() <= t.right() + 1e-3,
                    "横向不该跑出缩略图：滚动 {scroll}，框 {frame:?}，缩略图 {t:?}"
                );
                assert!(
                    frame.y >= t.y - 1e-3 && frame.bottom() <= t.bottom() + 1e-3,
                    "纵向不该跑出缩略图：滚动 {other}，框 {frame:?}，缩略图 {t:?}"
                );
            }
        }
    }

    /// 作品没溢出（可视区比作品还大）时，视口框就是整张缩略图。
    #[test]
    fn the_frame_covers_the_whole_thumbnail_when_nothing_overflows() {
        let t = thumb();
        // 可视区比作品大，说明一屏就能看全，框应盖满整张缩略图。
        let roomy = Size {
            width: artwork().width * 2.0,
            height: artwork().height * 2.0,
        };
        let frame = viewport_frame(t, artwork(), roomy, Point::new(0.0, 0.0));
        assert!(
            (frame.w - t.w).abs() < 1e-3 && (frame.h - t.h).abs() < 1e-3,
            "没溢出时框应盖满缩略图：框 {frame:?}，缩略图 {t:?}"
        );
        assert!(
            (frame.x - t.x).abs() < 1e-3 && (frame.y - t.y).abs() < 1e-3,
            "框应与缩略图对齐：框 {frame:?}，缩略图 {t:?}"
        );
    }

    /// 把框拖到缩略图右下角，滚动量换算成「作品右下角可见」。
    #[test]
    fn dragging_the_frame_to_the_corner_scrolls_to_the_end() {
        let t = thumb();
        let max = (artwork().width - viewport().width, artwork().height - viewport().height);
        let result = scroll_for_frame(
            Point::new(t.right(), t.bottom()),
            Point::new(0.0, 0.0),
            t,
            artwork(),
            viewport(),
            max,
        );
        assert!(
            (result.x - max.0).abs() < 1e-3,
            "拖到最右应滚到底：期望 {}，实际 {}",
            max.0,
            result.x
        );
        assert!(
            (result.y - max.1).abs() < 1e-3,
            "拖到最下应滚到底：期望 {}，实际 {}",
            max.1,
            result.y
        );
    }

    /// 拖动结果永远夹在有效范围内，拖出界也不会滚过头。
    #[test]
    fn dragging_beyond_the_edges_stays_within_range() {
        let t = thumb();
        let max = (artwork().width - viewport().width, artwork().height - viewport().height);
        for pointer in [Point::new(-500.0, -500.0), Point::new(900.0, 900.0)] {
            let result = scroll_for_frame(pointer, Point::new(0.0, 0.0), t, artwork(), viewport(), max);
            assert!(
                result.x >= 0.0 && result.x <= max.0,
                "横向应夹在范围内：{result:?}"
            );
            assert!(
                result.y >= 0.0 && result.y <= max.1,
                "纵向应夹在范围内：{result:?}"
            );
        }
    }

    /// 拖动全程保持按下时的抓取偏移，框不会在指针底下跳。
    #[test]
    fn the_grab_offset_is_kept_while_dragging() {
        let t = thumb();
        let max = (artwork().width - viewport().width, artwork().height - viewport().height);
        // 抓在框中心，往右挪 40 像素：已经滚过的距离应正好多出 40 / 缩放比例。
        let grab = Point::new(20.0, 10.0);
        let start = Point::new(t.x + 20.0, t.y + 10.0);
        let first = scroll_for_frame(start, grab, t, artwork(), viewport(), max);
        assert!(
            first.x.abs() < 1e-3 && first.y.abs() < 1e-3,
            "按在原处不该滚动：{first:?}"
        );
        let moved = Point::new(start.x + 40.0, start.y);
        let second = scroll_for_frame(moved, grab, t, artwork(), viewport(), max);
        let per_pixel = artwork().width / t.w;
        assert!(
            (second.x - 40.0 * per_pixel).abs() < 1e-2,
            "往右挪 40 像素应滚动 {} 个作品像素，实际 {}",
            40.0 * per_pixel,
            second.x
        );
    }

    /// 作品没溢出时不显示导航器。
    #[test]
    fn the_navigator_is_hidden_when_nothing_overflows() {
        let viewport = Size {
            width: gpui::px(800.),
            height: gpui::px(600.),
        };
        assert_eq!(
            navigator_rect(viewport, artwork(), (0.0, 0.0)),
            None,
            "作品没溢出时不该显示导航器"
        );
    }

    /// 任一方向溢出就显示，且贴在左下角。
    #[test]
    fn the_navigator_shows_at_the_bottom_left_when_either_axis_overflows() {
        let viewport = Size {
            width: gpui::px(800.),
            height: gpui::px(600.),
        };
        for max_scroll in [(10.0, 0.0), (0.0, 10.0), (10.0, 10.0)] {
            let rect = navigator_rect(viewport, artwork(), max_scroll)
                .unwrap_or_else(|| panic!("溢出 {max_scroll:?} 时应显示导航器"));
            assert!(rect.x < 800.0 / 2.0, "应贴左侧：{rect:?}");
            assert!(rect.y > 600.0 / 2.0, "应贴下侧：{rect:?}");
            assert!(rect.bottom() <= 600.0, "不该超出画布底部：{rect:?}");
            assert!(rect.right() <= 800.0, "不该超出画布右侧：{rect:?}");
        }
    }

    /// 导航器不和右下角的缩放胶囊重叠。
    #[test]
    fn the_navigator_never_overlaps_the_zoom_chip() {
        let viewport = Size {
            width: gpui::px(800.),
            height: gpui::px(600.),
        };
        let nav = navigator_rect(viewport, artwork(), (100.0, 100.0)).expect("应显示");
        // 缩放胶囊贴在右下角，尺寸约 168×34，离两边各 20 像素。
        let chip = Rect::new(800.0 - 20.0 - 168.0, 600.0 - 20.0 - 34.0, 168.0, 34.0);
        assert!(
            !nav.intersects(&chip),
            "导航器不该和缩放胶囊重叠：导航器 {nav:?}，胶囊 {chip:?}"
        );
    }

    /// 窗口小到放不下时不显示，不硬塞着盖住画布。
    #[test]
    fn the_navigator_is_hidden_in_a_window_too_small_for_it() {
        let tiny = Size {
            width: gpui::px(120.),
            height: gpui::px(600.),
        };
        assert_eq!(
            navigator_rect(tiny, artwork(), (500.0, 500.0)),
            None,
            "窗口放不下时不该显示导航器"
        );
    }
}

impl crate::app::AppRoot {
    /// 需求 15：画布左下角的全局预览导航器。
    ///
    /// 缩略图用 `CanvasLayout` 里每一块的矩形按比例摆放，位图取已有的预览缓存，
    /// 不合成全尺寸大图，也不新起渲染任务。上面盖一个视口框表示当前看到哪一块。
    ///
    /// 作品没超出可视区、或者窗口放不下时返回 `None`，整块不画。
    pub(crate) fn render_navigator(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        let (nav, thumb, frame) = self.navigator_geometry(viewport)?;
        let opacity = self.navigator_opacity_at(cx.background_executor().now());

        // 缩略图相对整张作品的比例：把每一块的窗口矩形换算到缩略图里。
        let layout = self.canvas_layout();
        let scale_x = if layout.width > 0.0 {
            thumb.w / layout.width
        } else {
            0.0
        };
        let scale_y = if layout.height > 0.0 {
            thumb.h / layout.height
        } else {
            0.0
        };

        let mut sheet = div()
            .debug_selector(|| "navigator-sheet".to_string())
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(thumb.w))
            .h(px(thumb.h))
            .overflow_hidden()
            .rounded(radius::SMALL)
            .bg(theme.surface_elevated);
        for entry in &layout.entries {
            if entry.is_placeholder {
                continue;
            }
            let x = entry.rect.x * scale_x;
            let y = entry.rect.y * scale_y;
            let w = (entry.rect.w * scale_x).max(1.0);
            let h = (entry.rect.h * scale_y).max(1.0);
            // 位图取缓存里已有的那张。没有就用淡色占位，不在这一帧生成新的。
            let id = entry.id;
            let layer: AnyElement = match self.cached_preview(id) {
                Some(image) => img(image)
                    .debug_selector(move || format!("navigator-block-{id}"))
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .object_fit(ObjectFit::Fill)
                    .into_any_element(),
                None => div()
                    .debug_selector(move || format!("navigator-block-{id}"))
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .bg(theme.muted.opacity(0.25))
                    .into_any_element(),
            };
            sheet = sheet.child(layer);
        }

        // 视口框：半透明底加一圈描边，一眼看出当前看到的是哪一块。
        let frame_box = div()
            .debug_selector(|| "navigator-frame".to_string())
            .id("navigator-frame")
            .absolute()
            .left(px(frame.x - thumb.x))
            .top(px(frame.y - thumb.y))
            .w(px(frame.w))
            .h(px(frame.h))
            .rounded(px(3.))
            .bg(theme.accent.opacity(0.18))
            .border_1()
            .border_color(theme.accent.opacity(0.9));

        let panel = div()
            .debug_selector(|| "navigator".to_string())
            .id("navigator")
            .absolute()
            .left(px(nav.x))
            .top(px(nav.y))
            .w(px(nav.w))
            .h(px(nav.h))
            .p(px(f32::from(tokens::NAVIGATOR_PAD)))
            // 缩略图按作品长宽比缩放后放进内框中央：横排的作品是宽而矮的，
            // 竖排是窄而高的，居中之后两侧留白一样宽，不会全堆在下边或右边。
            // 这里必须真的把缩略图摆在中间；只按缩略图尺寸摆一个盒子、不加
            // 居中，它会贴在内框左上角，看起来就是预览图下方或右侧一大片空白。
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::BLOCK)
            .bg(theme.surface.opacity(0.94))
            .border_1()
            .border_color(theme.border_subtle)
            .shadow(theme.shadow_bubble())
            .opacity(opacity)
            .cursor(CursorStyle::PointingHand)
            .on_hover(
                cx.listener(|this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget("navigator"), *hovered, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, event: &MouseDownEvent, window, cx| {
                    this.begin_navigator_drag(event, window, cx);
                    // 导航器浮在画布上，不掐断冒泡就会被当成“空白处拖窗口”。
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_drag(NavigatorDrag, |_value, _offset, _window, cx| {
                cx.new(|_| NavigatorDragGhost)
            })
            .on_drag_move::<NavigatorDrag>(cx.listener(
                |this: &mut AppRoot, event: &DragMoveEvent<NavigatorDrag>, window, cx| {
                    this.drag_navigator(event, window, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &MouseUpEvent, _window, cx| {
                    this.end_navigator_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this: &mut AppRoot, _: &MouseUpEvent, _window, cx| {
                    this.end_navigator_drag(cx);
                }),
            )
            .child(div().relative().w(px(thumb.w)).h(px(thumb.h)).child(sheet).child(frame_box));

        Some(panel.into_any_element())
    }
}

/// 拖导航器视口框时交给 GPUI 的类型标识，不需要跟手小图。
pub(crate) struct NavigatorDragGhost;

impl gpui::Render for NavigatorDragGhost {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
