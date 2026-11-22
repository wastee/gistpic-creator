//! 界面尺寸、字号、圆角与手势阈值常量。
//!
//! 界面里所有间距、字号、圆角、热区宽度都从这里取，不散落在各个文件里。
//! 改一个数，整个界面跟着变。

use gpui::{Pixels, Size, px};
use std::time::Duration;

/// 间距。只用 4 的倍数，避免出现 3、5、7 这类临时值。
pub mod space {
    use super::{Pixels, px};

    pub const XS: Pixels = px(4.);
    pub const SM: Pixels = px(8.);
    pub const MD: Pixels = px(12.);
    pub const LG: Pixels = px(16.);
    pub const XL: Pixels = px(24.);
}

/// 圆角。作品、浮层、胶囊、图块各一档。
pub mod radius {
    use super::{Pixels, px};

    /// 小块、图标按钮、输入框
    pub const SMALL: Pixels = px(6.);
    /// 拼合后的整张长图
    pub const WORK_CANVAS: Pixels = px(12.);
    /// 右键小卡片、空状态大卡片
    pub const CARD: Pixels = px(14.);
    /// 拉开缝隙后每个独立图块
    pub const BLOCK: Pixels = px(8.);
    /// 胶囊：999 等于全圆角
    pub const PILL: Pixels = px(999.);
}

/// 字号。只有三级。
pub mod text_size {
    use super::{Pixels, px};

    /// 正文（胶囊、菜单）
    pub const BODY: Pixels = px(13.);
    /// 说明文字、磁吸档位气泡、尺寸提示
    pub const CAPTION: Pixels = px(12.);
    /// 标题、空状态引导
    pub const TITLE: Pixels = px(14.);
}

/// 字重。
pub mod weight {
    use gpui::FontWeight;

    pub const REGULAR: FontWeight = FontWeight::NORMAL;
    pub const STRONG: FontWeight = FontWeight::SEMIBOLD;
}

/// 界面字体。中文和英文都用同一套，按顺序回退。
pub const FONT_FAMILY: &str = "Noto Sans";
/// 中文回退字体。
pub const FONT_FALLBACK_CJK: &str = "Noto Sans CJK SC";

/// 各部件的固定尺寸。
pub mod size {
    use super::{Pixels, px};

    /// 底部悬浮胶囊高度
    pub const CAPSULE: Pixels = px(46.);
    /// 胶囊左右内边距
    pub const CAPSULE_PAD: Pixels = px(10.);
    /// 胶囊离窗口底边的距离
    pub const CAPSULE_BOTTOM: Pixels = px(20.);
    /// 胶囊里的按钮高度
    pub const CAPSULE_BUTTON: Pixels = px(30.);
    /// 拖动时删除目标的高度与左右内边距（安静态）
    pub const DELETE_TARGET_H: Pixels = px(36.);
    pub const DELETE_TARGET_PAD: Pixels = px(12.);
    /// 鼠标压上去时删除目标的高度与左右内边距
    pub const DELETE_TARGET_ARMED_H: Pixels = px(44.);
    pub const DELETE_TARGET_ARMED_PAD: Pixels = px(16.);
    /// 删除目标里的垃圾桶图标边长
    pub const DELETE_TARGET_ICON: Pixels = px(18.);
    /// 胶囊里快捷比例小胶囊的高度
    pub const CHIP: Pixels = px(26.);
    /// 图标按钮的边长
    pub const ICON_BUTTON: Pixels = px(28.);
    /// 右键卡片宽度
    pub const MENU: Pixels = px(216.);
    /// 右键卡片每一行的高度
    pub const MENU_ROW: Pixels = px(32.);
    /// 空状态大卡片的最大宽度
    pub const EMPTY_CARD_MAX: Pixels = px(420.);
    /// 状态气泡与胶囊之间的间隙
    pub const TOAST_GAP: Pixels = px(8.);
    /// 画布底部预留的滚动留白，保证最后一张图不被胶囊挡住
    pub const CANVAS_BOTTOM_PAD: Pixels = px(96.);
    /// 裁剪线的粗细
    pub const CROP_LINE: Pixels = px(2.);
    /// 裁剪线中央握把的宽度与高度
    pub const CROP_GRIP_W: Pixels = px(28.);
    pub const CROP_GRIP_H: Pixels = px(6.);
    /// 边缘提示条的粗细：只有悬停/拖动的这一条边用细短圆角条，
    /// 剩下三条用更细的普通线，一眼能看出在调哪条边。
    pub const CROP_HINT_BAR: Pixels = px(3.);
    /// 边缘提示条的长度，取图宽的六成但不超过这个值。
    pub const CROP_HINT_LEN: Pixels = px(96.);
    /// 未激活的三条参照线粗细。
    pub const CROP_GHOST_LINE: Pixels = px(1.);
}

/// 画布左右留白：给作品左侧的缝隙控件让位。
///
/// 控件宽 28 像素、离作品左边缘 8 像素，占住约 36 像素。滚动区会裁掉
/// 超出内容范围的东西，所以左右各留 48 像素，控件不会被裁掉。
pub const CANVAS_SIDE_PAD: Pixels = px(48.);
/// 裁剪线的悬停热区厚度，从裁剪线往图内铺开。
pub const CROP_HOTZONE: Pixels = px(16.);
/// 两张图交界处多宽算接缝。有缝隙时按缝隙宽度收窄，最大不超过这个值的一半。
pub const SEAM_HOTZONE: Pixels = px(12.);
/// 两张图紧贴（间距为 0）时，接缝两侧各留这么宽的一段不接鼠标，合起来就是这一段。
///
/// 每侧 3 像素：上下相邻的图、左右相邻的图都一样。留这段死区是为了让人按在
/// 接缝附近时不会误按到隔壁那张图的边；间距大于 0 时不生效，热区照旧贴住
/// 裁剪线铺开。
pub const SEAM_DEADZONE: Pixels = px(6.);
/// 磁吸：鼠标离某个档位刻度多少像素以内就自动吸过去。
pub const SNAP_THRESHOLD_PX: f32 = 8.0;
/// 拉框多选：按下后至少移动这么多像素才算拉框，否则当成一次普通点击。
pub const MARQUEE_MIN_DRAG: f32 = 4.0;
/// 拖动图片时跟着鼠标走的那张小图，最长边的像素上限。
pub const DRAG_THUMB_SIDE: f32 = 88.0;
/// 拖动小图的透明度，压在别的图上也看得清底下。
pub const DRAG_THUMB_OPACITY: f32 = 0.85;

/// 拖动排序落点占位块的透明度：压暗但还看得见图，配合 2px 虚线框和中央
/// 「放这里」标签，让松手会落到哪一格一眼可见。
pub const DRAG_PLACEHOLDER_OPACITY: f32 = 0.62;

/// 选中图片的抬起、变淡和方勾动画。
pub mod selection {
    use super::Pixels;
    use gpui::px;

    /// 选中后统一的圆角。
    pub const RADIUS: Pixels = px(14.);
    /// 未选中图变淡的最终不透明度。
    pub const DIM: f32 = 0.45;
    /// 右上角方勾的边长。
    pub const CHECK: Pixels = px(18.);
    /// 方勾圆角。
    pub const CHECK_RADIUS: Pixels = px(4.);
    /// 方勾离抬起图右边、上边的距离。
    pub const CHECK_INSET: Pixels = px(8.);
    /// 选中时卡片圆角展开的用时。
    pub const CARD_IN_MS: u64 = 170;
    /// 取消选中时圆角收回的用时。
    pub const CARD_OUT_MS: u64 = 140;
    /// 其他图变淡用时。
    pub const DIM_IN_MS: u64 = 180;
    /// 其他图恢复用时。
    pub const DIM_OUT_MS: u64 = 160;
    /// 方勾出现前的等待。
    pub const CHECK_DELAY_MS: u64 = 30;
    /// 方勾出现用时。
    pub const CHECK_IN_MS: u64 = 120;
    /// 方勾消失用时。
    pub const CHECK_OUT_MS: u64 = 90;
    /// 一次选中切换最长用时，驱动任务按它停下。
    pub const TOTAL_MS: u64 = 200;
    /// 选中动画驱动任务的定时器间隔。16ms 约等于 60Hz，和屏幕刷新一致，
    /// 比原来 8ms 少一半唤醒，动画看起来一样顺。
    pub const SELECTION_TICK_MS: u64 = 16;
}

/// 拖动排序时指针进入视口边缘这么宽的带子就开始自动滚动。
pub const AUTO_SCROLL_EDGE: f32 = 96.0;
/// 自动滚动每一帧最大推进的像素；越靠边推进越快（见 `auto_scroll_intensity`）。
pub const AUTO_SCROLL_STEP: f32 = 14.0;
/// 自动滚动定时器间隔，约等于一帧。
pub const AUTO_SCROLL_INTERVAL_MS: u64 = 16;
/// 作品左侧的缝隙与颜色控件：竖药丸，上半是缝隙图标，下半是颜色圆点。
pub const GAP_CONTROL_W: Pixels = px(28.);
pub const GAP_CONTROL_H: Pixels = px(56.);
/// 控件里颜色圆点的直径。
pub const GAP_CONTROL_DOT: Pixels = px(16.);
/// 控件右边缘与作品左边缘的距离。
pub const GAP_CONTROL_GAP: Pixels = px(8.);
/// 缩放百分比小胶囊距画布右下角的距离。
pub const ZOOM_CHIP_GAP: Pixels = px(20.);

/// 边缘提示显现的时间：指针靠近图块、以及开始拖动时，高亮在这段时间里涨上来。
pub const CROP_HINT_FADE_IN: Duration = Duration::from_millis(120);
/// 松手之后边缘提示淡回默认状态的时间。
pub const CROP_HINT_FADE_OUT: Duration = Duration::from_millis(500);
/// 边缘提示动画的定时器间隔，约等于一帧。
pub const CROP_HINT_TICK: Duration = Duration::from_millis(16);
/// 边缘提示的默认状态：指针在图块上但没有拖动时的轻微高亮。
pub const CROP_HINT_REST: f32 = 0.35;
/// 拖动中裁剪短条和吸附标签的圆角，和选中方勾统一成 4 像素。
pub const CROP_BAR_RADIUS: f32 = 4.0;
/// 边缘提示完全淡出的强度：指针不在图块上。
pub const CROP_HINT_IDLE: f32 = 0.0;

/// 需求 12：间隔胶囊没被指针碰到时的不透明度。
pub const GAP_CONTROL_IDLE_OPACITY: f32 = 0.45;

/// 需求 11：间隔胶囊里那个「拖动按钮」的边长。
///
/// 按钮比方块小一圈，四周留出外沿：点在外沿上不启动拖动。
pub const GAP_CONTROL_HANDLE: Pixels = px(20.);

/// 需求 15：画布左下角全局预览导航器的外框尺寸。
pub const NAVIGATOR_W: Pixels = px(160.);
pub const NAVIGATOR_H: Pixels = px(120.);
/// 导航器没被指针碰到时的不透明度。
///
/// 需求 6：安静时更淡，只要还能看出缩略图的轮廓；指针压上去或拖框时变回不透明。
pub const NAVIGATOR_IDLE_OPACITY: f32 = 0.30;
/// 导航器内框四周的留白，缩略图摆在里面。
pub const NAVIGATOR_PAD: Pixels = px(10.);
/// 导航器底边离画布底部的距离：留出右下角缩放胶囊的位置，两块不叠。
pub const NAVIGATOR_BOTTOM: Pixels = px(84.);

/// 关闭按钮安静时的透明度，降低视觉存在感。
pub const CLOSE_IDLE_OPACITY: f32 = 0.12;
/// 关闭按钮“靠近就显示”的区域宽度：从窗口右边往左这么宽。
pub const CLOSE_REVEAL_W: f32 = 220.0;
/// 关闭按钮“靠近就显示”的区域高度：从窗口顶边往下这么高。
pub const CLOSE_REVEAL_H: f32 = 140.0;
/// 关闭按钮淡入淡出的时长。
pub const CLOSE_FADE: std::time::Duration = std::time::Duration::from_millis(150);
/// 淡入淡出期间驱动重绘的间隔，约等于一帧。
pub const CLOSE_FADE_TICK: std::time::Duration = std::time::Duration::from_millis(16);


/// 手动显示比例的允许范围：显示像素 / 输出像素。
///
/// 自动适应把整幅成品画布适配到窗口的可用区域，最大 100%、不放大；手动比例要能
/// 自己走得更远：1:1 看原尺寸，大图常常要缩到很小，小图也可能想放得比窗口还大。
/// 上限 4 倍足以看清细节；下限 0.1%，长截图缩到很小时也能一眼看到整张作品。
pub const MANUAL_SCALE_MIN: f32 = 0.001;
pub const MANUAL_SCALE_MAX: f32 = 4.0;

/// 按一次加减键或滚一格时，当前显示比例乘上的幅度。
pub const ZOOM_STEP: f32 = 0.1;

/// 画布显示比例的来源。
///
/// 界面上的百分比是「显示像素 / 成品输出像素」的真实比值，这个比值有两种来路：
/// 自动适应把整幅成品画布适配到窗口的可用区域，手动比例是用户选定的固定值。
/// 分开表达之后，1:1 才有一个明确、不随窗口和作品尺寸漂移的含义。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleMode {
    /// 自动适应：按整幅成品画布和窗口可用区域算出拟合比例，没有用户倍数。
    Fit,
    /// 手动比例：直接用「显示像素 / 输出像素」这个比值，不随窗口和作品尺寸重算。
    Manual(f32),
}

impl ScaleMode {
    /// 这个模式是不是自动适应。
    pub fn is_fit(self) -> bool {
        matches!(self, ScaleMode::Fit)
    }

    /// 手动比例的值；自动适应没有值。
    pub fn manual(self) -> Option<f32> {
        match self {
            ScaleMode::Fit => None,
            ScaleMode::Manual(scale) => Some(scale),
        }
    }
}

impl Default for ScaleMode {
    fn default() -> Self {
        ScaleMode::Fit
    }
}

/// 把手动显示比例收进允许的范围。
pub fn clamp_manual_scale(scale: f32) -> f32 {
    if scale.is_nan() {
        return 1.0;
    }
    scale.clamp(MANUAL_SCALE_MIN, MANUAL_SCALE_MAX)
}

/// 图片之间缝隙的范围。
pub const GAP_MIN: u32 = 0;
pub const GAP_MAX: u32 = 100;

/// 按住 Shift 精细调整时的减速倍数：数值变化速度是平时的 1/8。
///
/// 缝隙宽度和裁剪线共用。倍数只影响「鼠标走多远才动 1 个像素」，不改变 1 个
/// 像素本身的大小：缝隙按成品像素，裁剪按原图像素。
pub const FINE_ADJUST_FACTOR: f32 = 8.0;

/// 出血位的档数：0% 到 30%，每档 5%。
pub const BLEED_STEPS: usize = 7;
pub const BLEED_MAX: f32 = 0.30;

/// 把出血位比例换算成滑条档位下标。
pub fn bleed_to_index(bleed: f32) -> usize {
    let step = BLEED_MAX / (BLEED_STEPS - 1) as f32;
    ((bleed / step).round() as usize).min(BLEED_STEPS - 1)
}

/// 把滑条档位下标换算成出血位比例。
pub fn index_to_bleed(index: usize) -> f32 {
    let step = BLEED_MAX / (BLEED_STEPS - 1) as f32;
    (index.min(BLEED_STEPS - 1) as f32) * step
}


pub fn clamp_bleed(bleed: f32) -> f32 {
    if bleed.is_nan() {
        return BLEED_MAX / 2.0;
    }
    bleed.clamp(0.0, BLEED_MAX)
}

/// 当前画布显示比例的完整描述。
///
/// 界面上的百分比就是「显示像素 / 成品输出像素」乘 100：自动适应用
/// `fit_canvas_scale` 按整幅成品画布算，手动比例是用户定下的固定比值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewScale {
    pub mode: ScaleMode,
}

impl ViewScale {
    /// 自动适应：比例由整幅成品画布和窗口可用区域算出，没有用户倍数。
    pub fn fit() -> Self {
        ViewScale {
            mode: ScaleMode::Fit,
        }
    }

    /// 固定比例，就是「显示像素 / 输出像素」本身。
    pub fn manual(scale: f32) -> Self {
        ViewScale {
            mode: ScaleMode::Manual(clamp_manual_scale(scale)),
        }
    }

    pub fn is_fit(self) -> bool {
        self.mode.is_fit()
    }
}

/// 「适合画布」的比例：把整幅成品画布适配到窗口的可用区域。
///
/// 可用区域是窗口去掉左右各 `CANVAS_SIDE_PAD`、顶部 `space::XL`、底部
/// `size::CANVAS_BOTTOM_PAD` 之后剩下的矩形。可用宽和可用高各算一条比例，取更小的
/// 那个，在下限允许的范围内尽量把整幅画布放下；最大 100%，小图按原尺寸显示，不放大。
///
/// 返回值是「显示像素 / 输出像素」，收在 `MANUAL_SCALE_MIN..=1.0`：画布特别大时
/// 一直缩到 0.1%，不会停在某个更大的下限上。比例被 0.1% 截住时不再保证整幅可见，
/// 超出可用区域的部分靠滚动查看。画布尺寸无效、窗口还没量出尺寸（宽或高不是正数）、
/// 或者窗口小到可用区域被留白吃光（可用宽或可用高不是正数）时返回 1.0，按原尺寸显示。
pub fn fit_canvas_scale(canvas_w: f32, canvas_h: f32, viewport: Size<Pixels>) -> f32 {
    if !(canvas_w.is_finite() && canvas_w > 0.0 && canvas_h.is_finite() && canvas_h > 0.0) {
        return 1.0;
    }
    let view_w = f32::from(viewport.width);
    let view_h = f32::from(viewport.height);
    if !(view_w.is_finite() && view_w > 0.0 && view_h.is_finite() && view_h > 0.0) {
        return 1.0;
    }
    let available_w = view_w - 2.0 * f32::from(CANVAS_SIDE_PAD);
    let available_h = view_h - f32::from(space::XL) - f32::from(size::CANVAS_BOTTOM_PAD);
    // 窗口小到可用区域被留白吃光：没有地方放画布，按原尺寸显示，不要算出负比例再
    // 夹成 0.1%。
    if !(available_w.is_finite()
        && available_w > 0.0
        && available_h.is_finite()
        && available_h > 0.0)
    {
        return 1.0;
    }
    (available_w / canvas_w)
        .min(available_h / canvas_h)
        .min(1.0)
        .clamp(MANUAL_SCALE_MIN, 1.0)
}

pub fn clamp_gap_px(px: u32) -> u32 {
    px.clamp(GAP_MIN, GAP_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bleed_index_round_trips_through_every_step() {
        let step = BLEED_MAX / (BLEED_STEPS - 1) as f32;
        for index in 0..BLEED_STEPS {
            let bleed = index_to_bleed(index);
            assert!((bleed - step * index as f32).abs() < 1e-5);
            assert_eq!(bleed_to_index(bleed), index, "第 {index} 档换回来要一样");
        }
    }

    #[test]
    fn bleed_index_handles_out_of_range_input() {
        assert_eq!(bleed_to_index(-1.0), 0);
        assert_eq!(bleed_to_index(99.0), BLEED_STEPS - 1);
    }

    #[test]
    fn clamp_functions_reject_bad_values() {
        assert_eq!(clamp_manual_scale(f32::NAN), 1.0);
        assert_eq!(clamp_manual_scale(9.0), MANUAL_SCALE_MAX);
        assert_eq!(clamp_manual_scale(0.0), MANUAL_SCALE_MIN);
        // 下限放到 0.1%：原来的 0.5% 不再被夹，比下限还小的才夹。
        assert_eq!(clamp_manual_scale(0.005), 0.005);
        assert_eq!(clamp_manual_scale(0.000_5), MANUAL_SCALE_MIN);
        assert_eq!(clamp_gap_px(999), GAP_MAX);
        assert_eq!(clamp_bleed(-0.5), 0.0);
        assert_eq!(clamp_bleed(5.0), BLEED_MAX);
    }

    #[test]
    fn clamp_gap_px_accepts_100() {
        assert_eq!(GAP_MAX, 100);
        assert_eq!(clamp_gap_px(100), 100);
        assert_eq!(clamp_gap_px(101), 100);
    }

    /// 1381×804 的窗口：可用宽 1381 - 48×2 = 1285，可用高 804 - 24 - 96 = 684。
    fn window_1381x804() -> Size<Pixels> {
        Size {
            width: px(1381.),
            height: px(804.),
        }
    }

    fn available_area() -> (f32, f32) {
        let viewport = window_1381x804();
        (
            f32::from(viewport.width) - 2.0 * f32::from(CANVAS_SIDE_PAD),
            f32::from(viewport.height) - f32::from(space::XL) - f32::from(size::CANVAS_BOTTOM_PAD),
        )
    }

    /// 「适合画布」按整幅成品画布适配可用区域：宽和高各算一条，取更紧的那条。
    #[test]
    fn fit_canvas_scale_fits_the_whole_canvas_into_the_available_area() {
        let viewport = window_1381x804();
        let (available_w, available_h) = available_area();
        assert!((available_w - 1285.0).abs() < 1e-6);
        assert!((available_h - 684.0).abs() < 1e-6);

        // 1920×1080：宽度给 1285 ÷ 1920 ≈ 0.669，高度给 684 ÷ 1080 ≈ 0.633，
        // 高度这条更紧，取它。
        let scale = fit_canvas_scale(1920.0, 1080.0, viewport);
        assert!((scale - 684.0 / 1080.0).abs() < 1e-6, "实际 {scale}");
        assert!(
            (scale - 0.633_333_3).abs() < 1e-4,
            "1920×1080 单图应约等于 0.633，实际 {scale}"
        );
        assert!(1920.0 * scale <= available_w + 1e-3);
        assert!(
            (1080.0 * scale - available_h).abs() < 1e-3,
            "整幅画布的高度应正好贴住可用高"
        );

        // 200×100 的小图按原尺寸显示，不放大。
        assert_eq!(fit_canvas_scale(200.0, 100.0, viewport), 1.0);

        // 3840×21600 的竖长图：高度给 684 ÷ 21600 ≈ 0.03167，要真的缩到这么小。
        let scale = fit_canvas_scale(3840.0, 21600.0, viewport);
        assert!((scale - 684.0 / 21600.0).abs() < 1e-6, "实际 {scale}");
        assert!(
            (scale - 0.031_666_7).abs() < 1e-4,
            "整幅竖长图应约等于 0.03167，实际 {scale}"
        );
        assert!(21600.0 * scale <= available_h + 1e-3);
        assert!(3840.0 * scale <= available_w + 1e-3);
    }

    /// 画布或窗口尺寸无效时不做适配，按原尺寸显示。
    #[test]
    fn fit_canvas_scale_returns_full_size_for_invalid_sizes() {
        let viewport = window_1381x804();
        for (canvas_w, canvas_h) in [
            (0.0, 1080.0),
            (1920.0, 0.0),
            (-1920.0, 1080.0),
            (f32::NAN, 1080.0),
            (f32::INFINITY, 1080.0),
        ] {
            assert_eq!(
                fit_canvas_scale(canvas_w, canvas_h, viewport),
                1.0,
                "画布 {canvas_w}×{canvas_h} 无效，应返回 1.0"
            );
        }
        // 窗口还没量出尺寸：第一帧不能算出 0.1% 那么小的比例。
        let zero = Size {
            width: px(0.),
            height: px(0.),
        };
        assert_eq!(fit_canvas_scale(1920.0, 1080.0, zero), 1.0);
        // 窗口小到可用区域被留白吃光（可用宽或可用高不是正数）：没有地方放画布，
        // 按原尺寸显示，不能拿负数算比例再夹成 0.1%。
        for (width, height) in [(96.0, 804.0), (1381.0, 120.0), (50.0, 80.0)] {
            let tiny = Size {
                width: px(width),
                height: px(height),
            };
            assert_eq!(
                fit_canvas_scale(1920.0, 1080.0, tiny),
                1.0,
                "窗口 {width}×{height} 的可用区域不是正数，应返回 1.0"
            );
        }
    }

    /// 画布特别大时一直缩到手动比例的下限，不会停在更大的值上。
    #[test]
    fn fit_canvas_scale_can_shrink_down_to_the_manual_minimum() {
        let viewport = window_1381x804();
        // 可用高 684 ÷ 一千万 ≈ 0.0000684，比下限还小，收在 0.1%。
        assert_eq!(
            fit_canvas_scale(10_000_000.0, 10_000_000.0, viewport),
            MANUAL_SCALE_MIN
        );
        assert_eq!(MANUAL_SCALE_MIN, 0.001);
    }

    /// 手动比例就是显示像素与输出像素的比值，窗口和作品尺寸都改不动它。
    #[test]
    fn manual_scale_is_the_value_itself() {
        assert_eq!(ViewScale::manual(1.0).mode.manual(), Some(1.0));
        assert_eq!(ViewScale::manual(0.5).mode.manual(), Some(0.5));
        // 超出范围的值夹进允许区间
        assert_eq!(
            ViewScale::manual(99.0).mode.manual(),
            Some(MANUAL_SCALE_MAX)
        );
        assert_eq!(ViewScale::manual(0.0).mode.manual(), Some(MANUAL_SCALE_MIN));
        assert_eq!(ViewScale::manual(f32::NAN).mode.manual(), Some(1.0));
        // 两种来源能区分开：自动适应没有固定比值，手动比例有。
        assert!(ScaleMode::default().is_fit());
        assert!(ViewScale::fit().is_fit());
        assert!(!ViewScale::manual(0.5).is_fit());
        assert_eq!(ViewScale::fit().mode.manual(), None);
        assert_eq!(ScaleMode::Manual(0.25).manual(), Some(0.25));
    }

    #[test]
    fn clamp_manual_scale_rejects_bad_values() {
        assert_eq!(clamp_manual_scale(f32::NAN), 1.0);
        assert_eq!(clamp_manual_scale(0.0), MANUAL_SCALE_MIN);
        assert_eq!(clamp_manual_scale(9.0), MANUAL_SCALE_MAX);
        assert_eq!(clamp_manual_scale(0.75), 0.75);
        assert_eq!(clamp_manual_scale(0.001), 0.001);
        assert_eq!(clamp_manual_scale(0.0001), MANUAL_SCALE_MIN);
    }
}
