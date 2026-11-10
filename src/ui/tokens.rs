//! 界面尺寸与字号常量。
//!
//! 界面里所有间距、字号、圆角都从这里取，不散落在各个文件里。
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

/// 圆角。只有两档：小块和面板。
pub mod radius {
    use super::{Pixels, px};

    /// 按钮、小块、图标按钮
    pub const SMALL: Pixels = px(6.);
    /// 面板、浮层、菜单
    pub const LARGE: Pixels = px(10.);
}

/// 字号。只有三级。
pub mod text_size {
    use super::{Pixels, px};

    /// 正文
    pub const BODY: Pixels = px(13.);
    /// 说明文字
    pub const CAPTION: Pixels = px(12.);
    /// 标题
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

    /// 顶部工具栏高度
    pub const TOOLBAR: Pixels = px(48.);
    /// 底部状态栏高度
    pub const STATUS_BAR: Pixels = px(28.);
    /// 右边缘图标条收起时的宽度
    pub const RAIL: Pixels = px(40.);
    /// 图标条展开后的面板宽度
    pub const RAIL_PANEL: Pixels = px(260.);
    /// 图标条里每个图标的尺寸
    pub const RAIL_ICON: Pixels = px(40.);
    /// 工具栏按钮高度
    pub const BUTTON: Pixels = px(32.);
    /// 图标按钮的边长
    pub const ICON_BUTTON: Pixels = px(28.);
    /// 右键菜单宽度
    pub const MENU: Pixels = px(200.);
    /// 右键菜单每一行的高度
    pub const MENU_ROW: Pixels = px(30.);
    /// 浮层顶部标题栏高度
    pub const LAYER_HEADER: Pixels = px(48.);
    /// 浮层底部操作栏高度
    pub const LAYER_FOOTER: Pixels = px(40.);
    /// 画布右下角的拖拽手柄边长
    pub const RESIZE_HANDLE: Pixels = px(16.);
    /// 浮层里的边界线高度
    pub const BAND_LINE: Pixels = px(2.);
    /// 边界线上可拖动的热区高度
    pub const BAND_HIT: Pixels = px(14.);
}

/// 鼠标停在图标条上，等这么久才把参数面板弹出来。
///
/// 鼠标只是路过时不会闪一下面板。全软件只有这一处悬停延迟，
/// 觉得弹得快了慢了就改这一个数。
pub const HOVER_EXPAND_DELAY_MS: u64 = 200;

/// 预览缩放范围。
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

/// 一次滚轮调整缩放时改变的幅度。
pub const ZOOM_STEP: f32 = 0.1;

/// 图片之间缝隙的范围。
pub const GAP_MIN: u32 = 0;
pub const GAP_MAX: u32 = 40;
pub const GAP_STEP: u32 = 2;

/// 出血位的档数：0% 到 30%，每档 5%。
pub const BLEED_STEPS: usize = 7;
pub const BLEED_MAX: f32 = 0.30;

/// 多选叠放时最多画几层，超过的用一行文字代替。
pub const STACK_MAX_LAYERS: usize = 5;

/// 叠放的偏移量：每往下一层往右上错开多少像素。
pub const STACK_OFFSET: f32 = 8.0;
/// 叠放的缩放：每往下一层缩小多少比例。
pub const STACK_SCALE_STEP: f32 = 0.03;
/// 叠放的最小缩放，再往下不再缩小。
pub const STACK_SCALE_MIN: f32 = 0.88;
/// 叠放的透明度：最上面一层不动，每往下一层降低多少。
pub const STACK_OPACITY_STEP: f32 = 0.20;
/// 叠放的最小透明度。
pub const STACK_OPACITY_MIN: f32 = 0.20;

/// 各层图片的偏移、缩放、透明度。`depth` 为 0 表示最上面一层。
///
/// 这个函数不画东西，只算数，方便单独测试。
pub fn stack_layer(depth: usize) -> (f32, f32, f32) {
    if depth == 0 {
        return (0.0, 1.0, 1.0);
    }
    let depth = depth as f32;
    let offset = STACK_OFFSET * depth;
    let scale = (1.0 - STACK_SCALE_STEP * depth).max(STACK_SCALE_MIN);
    let opacity = (1.0 - STACK_OPACITY_STEP * depth).max(STACK_OPACITY_MIN);
    (offset, scale, opacity)
}

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
    fn stack_layers_offset_outward_and_fade() {
        let first = stack_layer(0);
        assert_eq!(first, (0.0, 1.0, 1.0));

        let second = stack_layer(1);
        assert!(second.0 > first.0, "第二层要比第一层往外错开");
        assert!(second.1 < first.1, "第二层要比第一层小");
        assert!(second.2 < first.2, "第二层要比第一层淡");

        let third = stack_layer(2);
        assert!(third.0 > second.0);
        assert!(third.1 < second.1);
        assert!(third.2 < second.2);
    }

    #[test]
    fn stack_layers_stop_shrinking_at_floor() {
        let deep = stack_layer(20);
        assert!(deep.1 >= STACK_SCALE_MIN - 1e-5);
        assert!(deep.2 >= STACK_OPACITY_MIN - 1e-5);
    }

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
}
