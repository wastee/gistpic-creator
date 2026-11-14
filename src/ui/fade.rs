//! 按时间推进的透明度动画。
//!
//! 界面里有几处「默认半透明、指针靠近才变清晰、离开后慢慢淡回去」的控件：
//! 裁剪边缘提示、作品左侧的间隔胶囊、画布左下角的全局预览。三处的时序一样，
//! 这里放一份实现，谁用谁持有一个 `Fade`。
//!
//! 时间从外面传进来，取值用 `cx.background_executor().now()`。这样无头测试里
//! 用假时钟推进就能验证动画，不用真的等上百毫秒。
//!
//! 动画只在真正过渡的那几百毫秒里按帧重画，播完驱动任务自己退出，
//! 不会一直占着主线程。

use std::time::{Duration, Instant};

use crate::app::AppRoot;
use gpui::Context;

/// 一段透明度动画的当前状态。
///
/// 记下起点值、起点时刻和目标值，取值时按经过的时间插值。
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    /// 动画起点的透明度；没有动画在跑时就是当前值。
    from: f32,
    /// 目标透明度。
    target: f32,
    /// 动画起点时刻；`None` 表示已经停在目标值上。
    started: Option<Instant>,
    /// 这段动画的总时长。
    duration: Duration,
    /// 代次：每换一次目标就加一，上一段的驱动任务看到代次变了就退出。
    generation: u64,
}

impl Fade {
    /// 没有动画在跑，稳定停在 `value`。
    pub fn new(value: f32) -> Self {
        Fade {
            from: value,
            target: value,
            started: None,
            duration: Duration::from_millis(1),
            generation: 0,
        }
    }

    /// 从当前的显示值开始，向 `target` 过渡。
    ///
    /// 目标没变且动画不在跑时什么也不做，返回 `false`，调用方不用重复启动驱动。
    pub fn toward(&mut self, target: f32, duration: Duration, now: Instant) -> bool {
        // 目标没变就不重开：正在往这个目标走的时候再叫一次，不能把进度打回起点。
        if self.target == target {
            return false;
        }
        self.from = self.value_at(now);
        self.target = target;
        self.duration = duration;
        self.started = Some(now);
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// 跳过动画，直接落在 `value` 上。
    pub fn settle(&mut self, value: f32) {
        self.from = value;
        self.target = value;
        self.started = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// `now` 这一刻的透明度。
    pub fn value_at(&self, now: Instant) -> f32 {
        let Some(started) = self.started else {
            return self.target;
        };
        let total = self.duration.as_secs_f32().max(f32::EPSILON);
        let ratio = (now.saturating_duration_since(started).as_secs_f32() / total).clamp(0.0, 1.0);
        // 先快后慢，看起来像轻轻滑到位，不会突然停住。
        let eased = 1.0 - (1.0 - ratio).powi(3);
        self.from + (self.target - self.from) * eased
    }

    /// 动画是否已经跑到终点。
    ///
    /// 按经过的时间判断，不比较数值大小：淡出时目标值比起点小，
    /// 用大小比较会一开始就认为播完了。
    pub fn is_settled_at(&self, now: Instant) -> bool {
        match self.started {
            Some(started) => now.saturating_duration_since(started) >= self.duration,
            None => true,
        }
    }

    /// 当前代次。驱动任务靠它判断自己是不是过期了。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 这段动画的时长，驱动任务按它定定时器间隔。
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

impl Default for Fade {
    fn default() -> Self {
        Fade::new(0.0)
    }
}

impl AppRoot {
    /// 按帧重画直到 `select` 选中的那段动画播完。
    ///
    /// `select` 用函数指针而不是闭包，这样驱动任务里不需要再借用 `self`。
    /// 中途换了目标（代次变了）就退出，由新的一次调用接管。
    pub(crate) fn drive_fade(&mut self, cx: &mut Context<Self>, select: fn(&AppRoot) -> &Fade) {
        let fade = select(self);
        let generation = fade.generation();
        let tick = fade
            .duration()
            .min(crate::ui::tokens::CROP_HINT_TICK)
            .max(Duration::from_millis(4));
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(tick).await;
                let stop = this
                    .update(cx, |root, cx| {
                        let now = cx.background_executor().now();
                        let fade = select(root);
                        if fade.generation() != generation {
                            return true;
                        }
                        cx.notify();
                        fade.is_settled_at(now)
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    /// 同一个基准时刻加上若干毫秒。
    ///
    /// 基准只能取一次：`Instant::now()` 每次调用都往前走，各算各的会让
    /// 时间差算错，测试就成了假失败。
    fn base() -> Instant {
        Instant::now()
    }

    fn at(base: Instant, millis: u64) -> Instant {
        base + ms(millis)
    }

    /// 刚建好就停在给定值上。
    #[test]
    fn a_fresh_fade_sits_on_its_value() {
        let base = base();
        let fade = Fade::new(0.28);
        assert!((fade.value_at(at(base, 0)) - 0.28).abs() < 1e-6);
        assert!(fade.is_settled_at(at(base, 0)));
    }

    /// 换目标会从当前值出发，不会跳。
    #[test]
    fn changing_the_target_starts_from_the_current_value() {
        let base = base();
        let mut fade = Fade::new(0.2);
        assert!(fade.toward(1.0, ms(120), at(base, 0)));
        assert!(
            (fade.value_at(at(base, 0)) - 0.2).abs() < 0.05,
            "刚开始时应该还在起点附近"
        );
        assert!(!fade.is_settled_at(at(base, 0)), "刚开始不该算播完");
    }

    /// 时间走完就到目标值。
    #[test]
    fn the_value_reaches_the_target_when_time_is_up() {
        let base = base();
        let mut fade = Fade::new(0.28);
        fade.toward(1.0, ms(120), at(base, 0));
        assert!(
            (fade.value_at(at(base, 120)) - 1.0).abs() < 1e-6,
            "到点应正好是目标值"
        );
        assert!(fade.is_settled_at(at(base, 120)));
    }

    /// 淡出时不该一开始就被当成播完。
    #[test]
    fn fading_out_is_not_settled_at_once() {
        let base = base();
        let mut fade = Fade::new(1.0);
        fade.toward(0.3, ms(500), at(base, 0));
        assert!(!fade.is_settled_at(at(base, 0)), "淡出刚开始不算播完");
        assert!(fade.value_at(at(base, 0)) > 0.9, "刚开始还在满格附近");
        assert!(fade.is_settled_at(at(base, 500)), "500 毫秒后播完");
        assert!((fade.value_at(at(base, 500)) - 0.3).abs() < 1e-6);
    }

    /// 同样的目标重复设置不会重开动画。
    #[test]
    fn asking_for_the_same_target_twice_is_ignored() {
        let base = base();
        let mut fade = Fade::new(0.2);
        assert!(fade.toward(1.0, ms(120), at(base, 0)));
        assert!(fade.is_settled_at(at(base, 120)));
        let generation = fade.generation();
        assert!(!fade.toward(1.0, ms(120), at(base, 200)), "已经停在目标上不该重开");
        assert_eq!(fade.generation(), generation);
    }

    /// 动画正在跑的时候再叫一次同样的目标，进度不能被打回起点。
    ///
    /// 指针每动一下都会重新报一次悬停，如果每次都重开动画，
    /// 高亮就会永远停在起点附近，看起来像根本没亮起来。
    #[test]
    fn a_running_animation_keeps_its_progress() {
        let base = base();
        let mut fade = Fade::new(0.0);
        assert!(fade.toward(1.0, ms(120), at(base, 0)));
        let midpoint = fade.value_at(at(base, 60));
        assert!(midpoint > 0.0 && midpoint < 1.0, "一半时间时应在中途");
        assert!(!fade.toward(1.0, ms(120), at(base, 60)), "已经在往这个目标走，不该重开");
        assert!(
            (fade.value_at(at(base, 60)) - midpoint).abs() < 1e-6,
            "进度不该被打回起点"
        );
        assert!(
            (fade.value_at(at(base, 120)) - 1.0).abs() < 1e-6,
            "还是按原来的起点算，到点仍应到目标值"
        );
    }

    /// 直接落值会取消动画。
    #[test]
    fn settling_cancels_the_animation() {
        let base = base();
        let mut fade = Fade::new(0.2);
        fade.toward(1.0, ms(500), at(base, 0));
        fade.settle(0.5);
        assert!((fade.value_at(at(base, 0)) - 0.5).abs() < 1e-6);
        assert!(fade.is_settled_at(at(base, 0)));
    }
}
