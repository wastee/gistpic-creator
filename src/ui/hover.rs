//! 全局悬停状态：指针进入就显示，离开就收起，不等待、不淡入。
//!
//! 界面上任何一处悬停都上报给 `HoverGate`；同一时刻最多一个目标处于激活状态。
//! 悬停只改颜色或透明度，不改元素的尺寸位置，避免指针扫过时布局跳动。
//!
//! 这里不再有延迟计时和淡入淡出：旧实现要等 200 毫秒才显示，又靠一个
//! 每 16 毫秒重画整个窗口的驱动任务推进淡入，既慢又持续占用主线程。

use gpui::{AnyElement, Context, IntoElement, SharedString};

use crate::app::AppRoot;
use crate::model::ItemId;

/// 语言二级菜单的收起等待时长。
///
/// 这不是显示延迟：二级菜单在指针进入“语言”一行的当帧就弹出；指针离开
/// “语言”行和子菜单后，再等这么久才收起，留出滑进子菜单的时间。
pub const SUBMENU_CLOSE_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

/// 一处可悬停的目标。界面只认 key 是否相等，不做比较运算。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HoverKey {
    /// 画布上的一个图块。
    Item(ItemId),
    /// 具名小部件：胶囊按钮、菜单行、缩放胶囊、关闭按钮、作品左侧的缝隙控件等。
    Widget(SharedString),
}

impl HoverKey {
    /// 用固定的部件名建 key。
    pub fn widget(name: &'static str) -> Self {
        HoverKey::Widget(SharedString::from(name))
    }
}

/// 悬停变化上报闭包的类型。`Box<dyn Fn>` 本身实现了 `Fn`，可以直接挂到 `on_hover`。
pub type HoverEvent = Box<dyn Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static>;

/// 小部件的悬停接入参数：是否激活，加上上报闭包。
///
/// 菜单行、按钮这类小部件拿这一个结构就够了，不用直接依赖全局门控。
pub struct HoverBinding {
    /// 现在是否应该画悬停反馈。
    pub active: bool,
    /// 悬停反馈进度：0.0 没有反馈，1.0 完全激活。现在只有这两个取值。
    pub fade: f32,
    /// 悬停变化上报闭包。
    pub on_event: HoverEvent,
}

impl HoverBinding {
    /// 一份永不激活、不上报的空绑定，给禁用的行用。
    pub fn idle() -> Self {
        HoverBinding {
            active: false,
            fade: 0.0,
            on_event: Box::new(|_, _, _| {}),
        }
    }
}

/// 悬停状态机：同一时刻最多一个目标处于激活状态。
#[derive(Default)]
pub struct HoverGate {
    /// 当前指针进入的目标。
    active: Option<HoverKey>,
}

impl HoverGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 这个目标现在是否应该画悬停反馈。
    pub fn is_active(&self, key: &HoverKey) -> bool {
        self.active.as_ref() == Some(key)
    }

    /// 这个目标是否被指针沾着。现在进入即激活，所以和 `is_active` 相同。
    /// 给“移开后要等一会儿再收起”的界面（比如二级菜单）用。
    pub fn is_engaged(&self, key: &HoverKey) -> bool {
        self.is_active(key)
    }

    /// 当前激活的图块，没有则返回 `None`.
    pub fn active_item(&self) -> Option<ItemId> {
        match self.active.as_ref() {
            Some(HoverKey::Item(id)) => Some(*id),
            _ => None,
        }
    }

    /// 仅供界面测试读取当前激活目标。
    #[cfg(test)]
    pub(crate) fn debug_active(&self) -> Option<HoverKey> {
        self.active.clone()
    }

    /// 目标此刻的悬停进度：激活就是 1.0，否则 0.0。
    pub fn fade_delta(&self, key: &HoverKey) -> f32 {
        if self.is_active(key) { 1.0 } else { 0.0 }
    }
}

impl AppRoot {
    /// 上报一次悬停变化。进入当帧就激活，离开当帧就收起。
    pub(crate) fn set_hover(&mut self, key: HoverKey, entered: bool, cx: &mut Context<Self>) {
        if entered {
            if self.hover_gate.active.as_ref() == Some(&key) {
                return;
            }
            self.hover_gate.active = Some(key);
            cx.notify();
        } else {
            if self.hover_gate.active.as_ref() != Some(&key) {
                return;
            }
            self.hover_gate.active = None;
            cx.notify();
        }
        // 悬停变化后联动依赖悬停的界面（语言二级菜单的弹出与延迟收起）。
        self.update_language_submenu(cx);
    }

    /// 造一个具名小部件的悬停上报闭包给 `on_hover` 用。
    pub(crate) fn widget_hover_listener(
        &self,
        name: &'static str,
        cx: &mut Context<Self>,
    ) -> impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static {
        cx.listener(move |this: &mut AppRoot, hovered: &bool, _window, cx| {
            this.set_hover(HoverKey::widget(name), *hovered, cx);
        })
    }

    /// 一份完整的小部件悬停接入参数，直接传给菜单行、按钮。
    pub(crate) fn hover_binding(&self, name: &'static str, cx: &mut Context<Self>) -> HoverBinding {
        let key = HoverKey::widget(name);
        HoverBinding {
            active: self.hover_gate.is_active(&key),
            fade: self.hover_gate.fade_delta(&key),
            on_event: Box::new(cx.listener(
                move |this: &mut AppRoot, hovered: &bool, _window, cx| {
                    this.set_hover(HoverKey::widget(name), *hovered, cx);
                },
            )),
        }
    }
}

/// 悬停淡入淡出的目标背景色。
///
/// 悬停是瞬时反馈，不能抹掉元素自身的状态指示：有底色时（比如选中
/// 预设的蓝色），悬停色按自身透明度乘以进度叠在底色上，底色始终可见；
/// 没有底色的元素照旧从透明淡入悬停色。
pub(crate) fn fade_background(
    base: Option<gpui::Hsla>,
    target: gpui::Hsla,
    fade: f32,
) -> gpui::Hsla {
    let Some(base) = base else {
        return target.opacity(fade);
    };
    if fade <= 0.0 {
        return base;
    }
    let mut mixed = crate::ui::theme::lerp_hsla(base, target, target.a * fade);
    mixed.a = base.a;
    mixed
}

/// 原样返回元素。
///
/// 保留这个名字，是因为画布上的透视层、裁剪线以前用动画包装层淡入；
/// 现在悬停即时显示，包装层会改变元素的动画 id 并影响点击命中，所以不再使用。
pub(crate) fn hover_fade<E>(element: E) -> AnyElement
where
    E: IntoElement + 'static,
{
    element.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn klein_blue() -> gpui::Hsla {
        gpui::rgb(0x2563eb).into()
    }

    fn hover_gray() -> gpui::Hsla {
        let black: gpui::Hsla = gpui::rgb(0x000000).into();
        black.opacity(0.04)
    }

    /// 进入当帧就激活，没有任何等待状态。
    #[test]
    fn hover_activates_immediately() {
        let mut gate = HoverGate::new();
        let key = HoverKey::widget("example");
        assert!(!gate.is_active(&key));
        gate.active = Some(key.clone());
        assert!(gate.is_active(&key));
        assert!(gate.is_engaged(&key));
        assert_eq!(gate.fade_delta(&key), 1.0);
        assert_eq!(gate.fade_delta(&HoverKey::widget("other")), 0.0);
    }

    /// 选中指示色是持久状态，悬停满格也只能轻微压暗，不能盖掉。
    #[test]
    fn hover_fade_never_erases_base_color() {
        let full = fade_background(Some(klein_blue()), hover_gray(), 1.0);
        assert!((full.a - 1.0).abs() < 1e-6, "悬停不能改变底色的不透明度");
        assert_ne!(full, hover_gray(), "悬停满格时选中蓝不能被换成悬停灰");
        assert_ne!(full, klein_blue(), "悬停满格时应有轻微的悬停反馈");

        let none = fade_background(Some(klein_blue()), hover_gray(), 0.0);
        assert_eq!(none, klein_blue(), "没有悬停进度时就是原底色");
    }

    /// 没有底色的元素照旧从透明淡入悬停色。
    #[test]
    fn fade_without_base_fades_in_target() {
        let zero = fade_background(None, hover_gray(), 0.0);
        assert!((zero.a - 0.0).abs() < 1e-6, "进度为零时全透明");

        let full = fade_background(None, hover_gray(), 1.0);
        assert!(
            (full.a - hover_gray().a).abs() < 1e-6,
            "淡入满格时到达悬停色的透明度"
        );
    }
}
