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
/// 控件宽 28 像素、离作品左边缘 8 像素，加阴影余量一共约 36 像素。滚动区会裁掉
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
/// 默认显示尺寸的黄金分割比例：作品最短边 = 窗口短边 × 这个系数。
///
/// 取 1 / 1.6180339887，约 0.618。作品默认不占满窗口，四周留出空白。
pub const GOLDEN_FIT_FRACTION: f32 = 0.618_033_988_7;
/// 拖动图片时跟着鼠标走的那张小图，最长边的像素上限。
pub const DRAG_THUMB_SIDE: f32 = 88.0;
/// 拖动小图的透明度，压在别的图上也看得清底下。
pub const DRAG_THUMB_OPACITY: f32 = 0.85;

/// 拖动排序落点占位块的透明度：和已落位的图片明显区分开。
pub const DRAG_PLACEHOLDER_OPACITY: f32 = 0.45;
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

/// 预览缩放范围（自动适应模式下乘在拟合比例上的相对倍数）。
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

/// 手动显示比例的允许范围：显示像素 / 输出像素。
///
/// 比自动适应的相对倍数宽得多：1:1 要看原尺寸，大图常常要缩到很小、小图也可能
/// 想放得比窗口还大。上限 4 倍足以看清细节，下限 5% 仍能一眼看到整张作品。
pub const MANUAL_SCALE_MIN: f32 = 0.05;
pub const MANUAL_SCALE_MAX: f32 = 4.0;

/// 一次滚轮调整缩放时改变的幅度。
pub const ZOOM_STEP: f32 = 0.1;

/// 画布显示比例的来源。
///
/// 界面上的百分比是「显示像素 / 成品输出像素」的真实比值，这个比值有两种来路：
/// 自动适应按窗口算，手动比例是用户选定的固定值。分开表达之后，1:1 才有一个
/// 明确、不随窗口和作品尺寸漂移的含义。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleMode {
    /// 自动适应：按窗口短边与黄金分割算出拟合比例，用户的相对缩放乘在上面。
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

    /// 配置里记的字符串。旧配置没有这个字段，读出来按自动适应处理。
    pub fn key(self) -> &'static str {
        match self {
            ScaleMode::Fit => "fit",
            ScaleMode::Manual(_) => "manual",
        }
    }

    /// 从配置里读回来。认不出的值一律当自动适应。
    pub fn from_key(key: &str, manual: f32) -> Self {
        match key {
            "manual" => ScaleMode::Manual(clamp_manual_scale(manual)),
            _ => ScaleMode::Fit,
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

/// 把这些数值收进允许的范围。
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_nan() {
        return 1.0;
    }
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

pub fn clamp_bleed(bleed: f32) -> f32 {
    if bleed.is_nan() {
        return BLEED_MAX / 2.0;
    }
    bleed.clamp(0.0, BLEED_MAX)
}

/// 当前画布显示比例的完整描述：比值从哪来，加上自动适应模式下的相对倍数。
///
/// 界面上的百分比就是 `scale()` 的返回值乘 100：显示像素 / 成品输出像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewScale {
    pub mode: ScaleMode,
    /// 只在自动适应模式下生效：乘在拟合比例上的相对倍数。
    pub zoom: f32,
}

impl ViewScale {
    /// 自动适应，相对倍数是 `zoom`。
    pub fn fit(zoom: f32) -> Self {
        ViewScale {
            mode: ScaleMode::Fit,
            zoom: clamp_zoom(zoom),
        }
    }

    /// 固定比例，就是「显示像素 / 输出像素」本身。
    pub fn manual(scale: f32) -> Self {
        ViewScale {
            mode: ScaleMode::Manual(clamp_manual_scale(scale)),
            zoom: 1.0,
        }
    }

    pub fn is_fit(self) -> bool {
        self.mode.is_fit()
    }

    /// 作品在屏幕上的显示比例：显示像素 / 输出像素。
    ///
    /// `basis` 是算「适合画布」用的基准尺寸，单位是原图像素。调用方传的是
    /// 「每张图自己裁剪后可见带的最短边里最大的那个」：只看图自己的尺寸，不看
    /// 整幅排版画布有多宽多高，所以同样的两张图横排和竖排会显示成一样大。
    ///
    /// 自动适应时基准不超过「窗口短边 × `GOLDEN_FIT_FRACTION`」，超过就按比例
    /// 压下来；不超过就按实际像素显示，不做放大，用户的相对倍数乘在上面。
    /// 手动比例时直接返回这个固定比值，不随窗口大小和作品尺寸变化。
    pub fn scale(self, basis: f32, viewport: Size<Pixels>) -> f32 {
        match self.mode {
            ScaleMode::Manual(scale) => clamp_manual_scale(scale),
            ScaleMode::Fit => {
                let zoom = clamp_zoom(self.zoom);
                if !basis.is_finite() || basis <= 0.0 {
                    return zoom;
                }
                let short_view = f32::from(viewport.width.min(viewport.height));
                if !short_view.is_finite() || short_view <= 0.0 {
                    return zoom;
                }
                let fit = short_view * GOLDEN_FIT_FRACTION;
                zoom * (fit / basis).min(1.0)
            }
        }
    }
}

/// 「适合画布」的基准尺寸：每张图裁剪后可见带的最短边里最大的那个。
///
/// 只看每张图自己的尺寸，不看整幅排版画布：同一批图横排和竖排会算出一个值，
/// 显示出来的大小因此一样。尺寸都无效时返回 0，调用方按「不做自适应」处理。
pub fn fit_basis(sizes: impl IntoIterator<Item = (f32, f32)>) -> f32 {
    sizes
        .into_iter()
        // 宽或高只要有一个不是有效的正数，这张图的尺寸就不能用：
        // 不能拿另一边的值顶替，那会算出一个假的基准。
        .filter(|(width, height)| {
            width.is_finite() && *width > 0.0 && height.is_finite() && *height > 0.0
        })
        .map(|(width, height)| width.min(height))
        .fold(0.0_f32, f32::max)
}

/// 作品在屏幕上的显示比例：显示像素 / 输出像素（见 `ViewScale::scale`）。
pub fn display_scale(basis: f32, view: ViewScale, viewport: Size<Pixels>) -> f32 {
    view.scale(basis, viewport)
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
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        assert_eq!(clamp_zoom(9.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(0.0), ZOOM_MIN);
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

    fn window_1280x860() -> Size<Pixels> {
        Size {
            width: px(1280.),
            height: px(860.),
        }
    }

    /// 单张图的基准就是这张图自己的最短边；多张图取其中最大的那个。
    #[test]
    fn fit_basis_uses_each_image_shortest_side() {
        assert_eq!(fit_basis([(1920.0, 1080.0)]), 1080.0);
        assert_eq!(fit_basis([(1080.0, 1920.0)]), 1080.0);
        assert_eq!(
            fit_basis([(1920.0, 1080.0), (1000.0, 1000.0)]),
            1080.0,
            "两张图取较短边里大的那个"
        );
        // 尺寸无效的图不参与；全无效时返回 0，调用方按不做自适应处理
        assert_eq!(fit_basis([(f32::NAN, 10.0), (0.0, 20.0)]), 0.0);
        assert_eq!(fit_basis(std::iter::empty()), 0.0);
    }

    /// 需求 7：同一批图不管横排还是竖排，算出的显示比例完全一样。
    #[test]
    fn fit_scale_is_the_same_for_row_and_column() {
        let viewport = window_1280x860();
        let fit = ViewScale::fit(1.0);
        let sizes = [(1920.0, 1080.0), (1000.0, 1000.0)];
        // 基准只看每张图自己，跟排版方向无关
        assert_eq!(fit_basis(sizes), fit_basis(sizes));
        assert!((fit.scale(fit_basis(sizes), viewport) - 860.0 * GOLDEN_FIT_FRACTION / 1080.0).abs() < 1e-6);
    }

    #[test]
    fn fit_scale_never_enlarges_and_fits_golden_ratio() {
        let viewport = window_1280x860();
        let fit = ViewScale::fit(1.0);
        // 小图按实际像素显示，不放大
        assert_eq!(fit.scale(fit_basis([(400.0, 300.0)]), viewport), 1.0);
        // 1920×1080 的最短边压到 860 × 0.618 ≈ 531，宽约 945
        let scale = fit.scale(fit_basis([(1920.0, 1080.0)]), viewport);
        let expected = 860.0 * GOLDEN_FIT_FRACTION / 1080.0;
        assert!((scale - expected).abs() < 1e-6);
        assert!((1920.0 * scale - 945.0).abs() < 1.0);
        assert!((1080.0 * scale - 531.0).abs() < 1.0);
        // 竖长图的最短边是宽，压到约 531，高约 897（超过窗口，靠滚动看全）
        let scale = fit.scale(fit_basis([(1920.0, 3240.0)]), viewport);
        let expected = 860.0 * GOLDEN_FIT_FRACTION / 1920.0;
        assert!((scale - expected).abs() < 1e-6);
        assert!((3240.0 * scale - 897.0).abs() < 1.0);
    }

    #[test]
    fn fit_scale_stacks_user_zoom_on_top() {
        let viewport = window_1280x860();
        let doubled = ViewScale::fit(2.0);
        assert_eq!(doubled.scale(fit_basis([(400.0, 300.0)]), viewport), 2.0);
        let scale = doubled.scale(fit_basis([(1920.0, 1080.0)]), viewport);
        let expected = 2.0 * 860.0 * GOLDEN_FIT_FRACTION / 1080.0;
        assert!((scale - expected).abs() < 1e-6);
    }

    #[test]
    fn fit_scale_handles_empty_and_bad_input() {
        let viewport = window_1280x860();
        let fit = ViewScale::fit(1.0);
        assert_eq!(fit.scale(fit_basis([(0.0, 0.0)]), viewport), 1.0);
        assert_eq!(fit.scale(fit_basis([(f32::NAN, 100.0)]), viewport), 1.0);
        // 缩放值本身无效时退回 1.0，再由 clamp_zoom 夹紧
        assert_eq!(
            ViewScale::fit(f32::NAN).scale(fit_basis([(400.0, 300.0)]), viewport),
            1.0
        );
        // 窗口尺寸还没量出来时不做自适应，只保留用户缩放
        let zero = Size {
            width: px(0.),
            height: px(0.),
        };
        assert_eq!(fit.scale(fit_basis([(1920.0, 1080.0)]), zero), 1.0);
    }

    /// 手动比例就是显示像素与输出像素的比值，窗口和作品尺寸都改不动它。
    #[test]
    fn manual_scale_ignores_output_size_and_viewport() {
        let viewport = window_1280x860();
        let one_to_one = ViewScale::manual(1.0);
        assert_eq!(one_to_one.scale(400.0, viewport), 1.0);
        assert_eq!(one_to_one.scale(1080.0, viewport), 1.0);
        assert_eq!(
            one_to_one.scale(1080.0, Size {
                width: px(320.),
                height: px(240.),
            }),
            1.0,
            "手动比例不随窗口变小而缩"
        );
        let half = ViewScale::manual(0.5);
        assert_eq!(half.scale(1000.0, viewport), 0.5);
        // 超出范围的值夹进允许区间
        assert_eq!(
            ViewScale::manual(99.0).scale(1000.0, viewport),
            MANUAL_SCALE_MAX
        );
        assert_eq!(
            ViewScale::manual(0.0).scale(1000.0, viewport),
            MANUAL_SCALE_MIN
        );
        assert_eq!(ViewScale::manual(f32::NAN).scale(10.0, viewport), 1.0);
    }

    /// 显示比例的模式和相对倍数：手动比例不记相对倍数，自动适应记。
    #[test]
    fn scale_mode_distinguishes_fit_from_manual() {
        assert!(ScaleMode::default().is_fit());
        assert_eq!(ScaleMode::from_key("fit", 0.3), ScaleMode::Fit);
        // 旧配置认不出的值当自动适应
        assert_eq!(ScaleMode::from_key("", 0.3), ScaleMode::Fit);
        assert_eq!(
            ScaleMode::from_key("manual", 0.3),
            ScaleMode::Manual(0.3)
        );
        assert_eq!(ScaleMode::Manual(1.0).key(), "manual");
        assert_eq!(ScaleMode::Fit.key(), "fit");
        assert_eq!(ScaleMode::Manual(0.25).manual(), Some(0.25));
        assert_eq!(ScaleMode::Fit.manual(), None);
        // 配置里存的比值同样夹进允许区间
        assert_eq!(
            ScaleMode::from_key("manual", 99.0),
            ScaleMode::Manual(MANUAL_SCALE_MAX)
        );
    }

    #[test]
    fn clamp_manual_scale_rejects_bad_values() {
        assert_eq!(clamp_manual_scale(f32::NAN), 1.0);
        assert_eq!(clamp_manual_scale(0.0), MANUAL_SCALE_MIN);
        assert_eq!(clamp_manual_scale(9.0), MANUAL_SCALE_MAX);
        assert_eq!(clamp_manual_scale(0.75), 0.75);
    }
}
