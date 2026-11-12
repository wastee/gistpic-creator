//! 界面尺寸、字号、圆角与手势阈值常量。
//!
//! 界面里所有间距、字号、圆角、热区宽度都从这里取，不散落在各个文件里。
//! 改一个数，整个界面跟着变。

use gpui::{Pixels, Size, px};

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
    /// 画布右下角的拖拽手柄边长
    pub const RESIZE_HANDLE: Pixels = px(24.);
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
/// 作品左侧的缝隙与颜色控件：竖药丸，上半是缝隙图标，下半是颜色圆点。
pub const GAP_CONTROL_W: Pixels = px(28.);
pub const GAP_CONTROL_H: Pixels = px(56.);
/// 控件里颜色圆点的直径。
pub const GAP_CONTROL_DOT: Pixels = px(16.);
/// 控件右边缘与作品左边缘的距离。
pub const GAP_CONTROL_GAP: Pixels = px(8.);
/// 缩放百分比小胶囊距画布右下角的距离。
pub const ZOOM_CHIP_GAP: Pixels = px(20.);

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

/// 预览缩放范围。
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

/// 一次滚轮调整缩放时改变的幅度。
pub const ZOOM_STEP: f32 = 0.1;

/// 图片之间缝隙的范围。
pub const GAP_MIN: u32 = 0;
pub const GAP_MAX: u32 = 40;

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

/// 作品在屏幕上的显示比例：显示像素 / 输出像素。
///
/// 作品最短边默认不超过「窗口短边 × `GOLDEN_FIT_FRACTION`」，超过就按比例压下来；
/// 不超过就按实际像素显示，不做放大。用户的缩放比例乘在自适应比例之上。
pub fn display_scale(
    output_width: f32,
    output_height: f32,
    zoom: f32,
    viewport: Size<Pixels>,
) -> f32 {
    let zoom = clamp_zoom(zoom);
    let shortest = output_width.min(output_height);
    if !shortest.is_finite() || shortest <= 0.0 {
        return zoom;
    }
    let short_view = f32::from(viewport.width.min(viewport.height));
    if !short_view.is_finite() || short_view <= 0.0 {
        return zoom;
    }
    let fit = short_view * GOLDEN_FIT_FRACTION;
    zoom * (fit / shortest).min(1.0)
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

    fn window_1280x860() -> Size<Pixels> {
        Size {
            width: px(1280.),
            height: px(860.),
        }
    }

    #[test]
    fn display_scale_never_enlarges_and_fits_golden_ratio() {
        let viewport = window_1280x860();
        // 小图按实际像素显示，不放大
        assert_eq!(display_scale(400.0, 300.0, 1.0, viewport), 1.0);
        // 1920×1080 的最短边压到 860 × 0.618 ≈ 531，宽约 945
        let scale = display_scale(1920.0, 1080.0, 1.0, viewport);
        let expected = 860.0 * GOLDEN_FIT_FRACTION / 1080.0;
        assert!((scale - expected).abs() < 1e-6);
        assert!((1920.0 * scale - 945.0).abs() < 1.0);
        assert!((1080.0 * scale - 531.0).abs() < 1.0);
        // 竖长图的最短边是宽，压到约 531，高约 897（超过窗口，靠滚动看全）
        let scale = display_scale(1920.0, 3240.0, 1.0, viewport);
        let expected = 860.0 * GOLDEN_FIT_FRACTION / 1920.0;
        assert!((scale - expected).abs() < 1e-6);
        assert!((3240.0 * scale - 897.0).abs() < 1.0);
    }

    #[test]
    fn display_scale_stacks_user_zoom_on_top() {
        let viewport = window_1280x860();
        assert_eq!(display_scale(400.0, 300.0, 2.0, viewport), 2.0);
        let scale = display_scale(1920.0, 1080.0, 2.0, viewport);
        let expected = 2.0 * 860.0 * GOLDEN_FIT_FRACTION / 1080.0;
        assert!((scale - expected).abs() < 1e-6);
    }

    #[test]
    fn display_scale_handles_empty_and_bad_input() {
        let viewport = window_1280x860();
        assert_eq!(display_scale(0.0, 0.0, 1.0, viewport), 1.0);
        assert_eq!(display_scale(f32::NAN, 100.0, 1.0, viewport), 1.0);
        // 缩放值本身无效时退回 1.0，再由 clamp_zoom 夹紧
        assert_eq!(display_scale(400.0, 300.0, f32::NAN, viewport), 1.0);
        // 窗口尺寸还没量出来时不做自适应，只保留用户缩放
        let zero = Size {
            width: px(0.),
            height: px(0.),
        };
        assert_eq!(display_scale(1920.0, 1080.0, 1.0, zero), 1.0);
    }
}
