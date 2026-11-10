//! 界面尺寸、字号、圆角与手势阈值常量。
//!
//! 界面里所有间距、字号、圆角、热区宽度都从这里取，不散落在各个文件里。
//! 改一个数，整个界面跟着变。

use gpui::{Pixels, px};

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

/// 鼠标离裁剪线多近时开始透视原图、出现可拖动热区。
pub const CROP_HOTZONE: Pixels = px(14.);
/// 两张图交界处多宽算接缝。
pub const SEAM_HOTZONE: Pixels = px(12.);
/// 磁吸：鼠标离某个档位刻度多少像素以内就自动吸过去。
pub const SNAP_THRESHOLD_PX: f32 = 8.0;
/// 拉框多选：按下后至少移动这么多像素才算拉框，否则当成一次普通点击。
pub const MARQUEE_MIN_DRAG: f32 = 4.0;
/// 画布内容的基准宽度（缩放 100% 时的宽度）。
pub const CANVAS_BASE_WIDTH: f32 = 560.0;
/// 缩放百分比小胶囊距画布右下角的距离。
pub const ZOOM_CHIP_GAP: Pixels = px(20.);

/// 预览缩放范围。
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

/// 一次滚轮调整缩放时改变的幅度。
pub const ZOOM_STEP: f32 = 0.1;

/// 图片之间缝隙的范围。
pub const GAP_MIN: u32 = 0;
pub const GAP_MAX: u32 = 40;
pub const GAP_STEP: u32 = 2;

/// 拖接缝时，鼠标移动多少像素等于一像素缝隙。
///
/// 画布放大到 200% 时，屏幕上的 2 像素才等于图片里的 1 像素，所以这里要除以缩放比例。
pub fn seam_drag_to_gap(drag_px: f32, zoom: f32) -> u32 {
    let zoom = clamp_zoom(zoom);
    clamp_gap_px((drag_px / zoom).round().max(0.0) as u32)
}

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
    fn seam_drag_maps_pixels_to_gap() {
        // 100% 缩放时屏幕像素就是图片像素
        assert_eq!(seam_drag_to_gap(12.0, 1.0), 12);
        // 200% 缩放时屏幕上的 24 像素只等于图片里的 12 像素
        assert_eq!(seam_drag_to_gap(24.0, 2.0), 12);
        // 上限 40 像素夹紧
        assert_eq!(seam_drag_to_gap(400.0, 1.0), GAP_MAX);
        // 往上推不会是负数
        assert_eq!(seam_drag_to_gap(-30.0, 1.0), GAP_MIN);
    }

    #[test]
    fn seam_drag_survives_bad_zoom() {
        assert_eq!(seam_drag_to_gap(10.0, f32::NAN), 10);
    }
}
