//! 全局悬停延迟：所有悬停反馈先等 200 毫秒，再淡入。
//!
//! 界面上任何一处悬停都上报给 `HoverGate`；目标要连续悬停满 `HOVER_DELAY`
//! 才会进入激活状态，期间界面不做任何视觉变化。离开目标立即取消，不留尾巴。
//!
//! 淡入淡出不用动画包装层实现：包装层带自己的动画 id，会改写按钮在
//! GPUI 里的元素状态路径；快速点击时“已按下待抬起”的记录存在按钮的
//! 元素状态里，路径一变记录就丢，点击会被吞掉。现在由 `HoverGate`
//! 记下每次淡入淡出的起点，界面渲染时按时间直接算出进度，
//! 元素树的结构始终保持稳定。

use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, ElementId, IntoElement, SharedString,
    Window, ease_out_quint,
};

use crate::app::AppRoot;
use crate::model::ItemId;

/// 悬停反馈生效前要等待的时长。
pub const HOVER_DELAY: Duration = Duration::from_millis(200);
/// 悬停反馈淡入动画的时长。
pub const HOVER_FADE: Duration = Duration::from_millis(150);
/// 悬停反馈淡出动画的时长。出场要比入场更快。
pub const HOVER_FADE_OUT: Duration = Duration::from_millis(100);
/// 淡入淡出期间驱动重绘的间隔，约等于一帧。
const FADE_TICK: Duration = Duration::from_millis(16);

/// 一处可悬停的目标。界面只认 key 是否相等，不做比较运算。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HoverKey {
    /// 画布上的一个图块。
    Item(ItemId),
    /// 两张图之间的一条接缝。
    Seam(usize),
    /// 具名小部件：胶囊按钮、菜单行、缩放胶囊、关闭按钮等。
    Widget(SharedString),
}

impl HoverKey {
    /// 用固定的部件名建 key。
    pub fn widget(name: &'static str) -> Self {
        HoverKey::Widget(SharedString::from(name))
    }
}

/// 悬停变化上报闭包的类型。`Box<dyn Fn>` 本身实现了 `Fn`，可以直接挂到 `on_hover`。
pub type HoverEvent = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// 小部件的悬停接入参数：是否激活、当前淡入淡出进度，加上上报闭包。
///
/// 菜单行、按钮这类小部件拿这一个结构就够了，不用直接依赖全局门控。
pub struct HoverBinding {
    /// 现在是否应该画悬停反馈。
    pub active: bool,
    /// 当前淡入淡出进度：0.0 没有反馈，1.0 完全激活。
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

/// 一次淡入或淡出的起点与方向。
#[derive(Clone, Copy)]
struct Fade {
    started: Instant,
    /// true 表示淡入到完全激活；false 表示从激活淡出到消失。
    to_full: bool,
}

/// 悬停状态机：同一时刻最多一个目标处于激活状态。
#[derive(Default)]
pub struct HoverGate {
    /// 已经等满延迟、正在显示反馈的目标。
    active: Option<HoverKey>,
    /// 还在等延迟的目标。
    waiting: Option<HoverKey>,
    /// 每次状态变化都加一，用来让旧的计时任务作废.
    generation: u64,
    /// 正在播放的淡入淡出。同一目标最多一条记录，新记录覆盖旧记录。
    fades: Vec<(HoverKey, Fade)>,
}

impl HoverGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 这个目标现在是否应该画悬停反馈。
    pub fn is_active(&self, key: &HoverKey) -> bool {
        self.active.as_ref() == Some(key)
    }

    /// 这个目标是否被指针沾着：已激活，或还在等延迟。
    /// 给“移开后要等一会儿再收起”的界面（比如二级菜单）用。
    pub fn is_engaged(&self, key: &HoverKey) -> bool {
        self.active.as_ref() == Some(key) || self.waiting.as_ref() == Some(key)
    }

    /// 当前激活的图块，没有则返回 `None`.
    pub fn active_item(&self) -> Option<ItemId> {
        match self.active.as_ref() {
            Some(HoverKey::Item(id)) => Some(*id),
            _ => None,
        }
    }

    /// 仅供界面冒烟测试读取当前激活目标。
    #[cfg(test)]
    pub(crate) fn debug_active(&self) -> Option<HoverKey> {
        self.active.clone()
    }

    /// 记一次淡入或淡出，从 `now` 开始播。同一目标的旧记录直接覆盖。
    pub fn start_fade(&mut self, key: HoverKey, to_full: bool, now: Instant) {
        match self.fades.iter_mut().find(|(k, _)| k == &key) {
            Some((_, fade)) => *fade = Fade { started: now, to_full },
            None => self.fades.push((key, Fade { started: now, to_full })),
        }
    }

    /// 立刻结束某个目标的淡入淡出。指针重新进入时用，避免残留半截反馈。
    pub fn clear_fade(&mut self, key: &HoverKey) {
        self.fades.retain(|(k, _)| k != key);
    }

    /// 目标此刻的淡入淡出进度：0.0 没有反馈，1.0 完全激活。
    ///
    /// 没有播放记录时，激活中的目标就是完全激活，其余目标就是零。
    pub fn fade_delta(&self, key: &HoverKey, now: Instant) -> f32 {
        let Some((_, fade)) = self.fades.iter().find(|(k, _)| k == key) else {
            return if self.is_active(key) { 1.0 } else { 0.0 };
        };
        let total = if fade.to_full {
            HOVER_FADE
        } else {
            HOVER_FADE_OUT
        };
        let ratio = (now.duration_since(fade.started).as_secs_f32() / total.as_secs_f32())
            .clamp(0.0, 1.0);
        // 和原实现一致，用 ease_out_quint 曲线。
        let eased = 1.0 - (1.0 - ratio).powi(5);
        if fade.to_full {
            eased
        } else {
            1.0 - eased
        }
    }

    /// 清掉已经播完的记录，驱动任务据此判断什么时候可以停。
    pub fn retire_finished_fades(&mut self, now: Instant) {
        self.fades.retain(|(_, fade)| {
            let total = if fade.to_full {
                HOVER_FADE
            } else {
                HOVER_FADE_OUT
            };
            now.duration_since(fade.started) < total
        });
    }
}

impl AppRoot {
    /// 上报一次悬停变化。进入时启动延迟计时，离开时立即清除。
    pub(crate) fn set_hover(&mut self, key: HoverKey, entered: bool, cx: &mut Context<Self>) {
        if entered {
            // 重新进入：立刻结束自己的淡出，避免残留半截反馈。
            self.hover_gate.clear_fade(&key);
            // 换目标了：原来激活的目标马上开始淡出，等延时的目标作废。
            if let Some(previous) = self.hover_gate.active.clone().filter(|previous| *previous != key) {
                let now = Instant::now();
                self.hover_gate.start_fade(previous, false, now);
                self.start_fade_driver(cx);
            }
            let already = self.hover_gate.active.as_ref() == Some(&key)
                || self.hover_gate.waiting.as_ref() == Some(&key);
            if already {
                return;
            }
            self.hover_gate.active = None;
            self.hover_gate.waiting = Some(key.clone());
            self.hover_gate.generation += 1;
            let generation = self.hover_gate.generation;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HOVER_DELAY).await;
                let _ = this.update(cx, |root, cx| {
                    if root.hover_gate.generation == generation
                        && root.hover_gate.waiting.as_ref() == Some(&key)
                    {
                        root.hover_gate.waiting = None;
                        let now = Instant::now();
                        root.hover_gate.active = Some(key.clone());
                        root.hover_gate.start_fade(key.clone(), true, now);
                        root.start_fade_driver(cx);
                        cx.notify();
                        // 到点激活也要联动依赖悬停的界面（语言二级菜单）。
                        root.update_language_submenu(cx);
                    }
                });
            })
            .detach();
        } else {
            // 只在真的清掉自己的等待时才推进代数：指针从图 A 滑到图 B 时，
            // 两条事件可能先来 B 的进入、后来 A 的离开，A 不能误杀 B 的计时。
            let had_active = self.hover_gate.active.as_ref() == Some(&key);
            let had_waiting = self.hover_gate.waiting.as_ref() == Some(&key);
            if had_active {
                self.hover_gate.active = None;
                let now = Instant::now();
                self.hover_gate.start_fade(key, false, now);
                self.start_fade_driver(cx);
            }
            if had_waiting {
                self.hover_gate.waiting = None;
                self.hover_gate.generation += 1;
            }
            if had_active || had_waiting {
                cx.notify();
            }
        }
        // 悬停变化后联动依赖悬停的界面（语言二级菜单的弹出与延迟收起）。
        self.update_language_submenu(cx);
    }

    /// 淡入淡出期间持续重绘的驱动任务。全局最多一个，播完自动退出。
    ///
    /// 旧实现靠动画包装层请求帧；现在进度是按时间算出来的，需要这里
    /// 定期触发重绘，直到所有淡入淡出播完。
    pub(crate) fn start_fade_driver(&mut self, cx: &mut Context<Self>) {
        if self.fade_driver_running {
            return;
        }
        self.fade_driver_running = true;
        self.fade_driver_gen += 1;
        let generation = self.fade_driver_gen;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FADE_TICK).await;
                let mut stop = false;
                let update_ok = this
                    .update(cx, |root, cx| {
                        if root.fade_driver_gen != generation {
                            // 已有更新的驱动接管，这一份直接退出。
                            stop = true;
                            return;
                        }
                        root.hover_gate.retire_finished_fades(Instant::now());
                        if root.hover_gate.fades.is_empty() {
                            root.fade_driver_running = false;
                            stop = true;
                        }
                        cx.notify();
                    })
                    .is_ok();
                if stop || !update_ok {
                    break;
                }
            }
        })
        .detach();
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
            fade: self.hover_gate.fade_delta(&key, Instant::now()),
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
/// 没有底色的元素照旧从透明淡入悬停色。旧实现直接向目标色插值，
/// 悬停满格时选中态的蓝色会被悬停灰整个盖掉，看起来像选中丢了。
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

/// 激活时给元素包一层淡入动画；未激活时原样返回。
///
/// 只用于包裹画布透视层、裁剪线这类不含可点元素的叶子内容；
/// 可点元素的悬停反馈走 `fade_delta` 计算，不要用包装层，
/// 否则动画 id 会改变按钮的元素状态路径，快速点击会被吞掉。
pub(crate) fn hover_fade<E, F>(
    animate: bool,
    id: impl Into<ElementId>,
    element: E,
    fade: F,
) -> AnyElement
where
    E: IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    if animate {
        element
            .with_animation(
                id,
                Animation::new(HOVER_FADE).with_easing(ease_out_quint()),
                fade,
            )
            .into_any_element()
    } else {
        element.into_any_element()
    }
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
