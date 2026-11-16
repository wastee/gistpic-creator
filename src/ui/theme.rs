//! 界面配色：深色画板风格。
//!
//! 整个窗口是一张深色画板，作品、胶囊、右键卡片浮在上面。
//! 只用两种彩色：克莱因蓝表示强调和可操作，珊瑚红表示删除。
//! 只有这一套深色配色，不跟随系统外观切换。
//! 想调整观感就改这里的色值，所有界面都会跟着变。

use gpui::Hsla;

#[derive(Clone, Copy)]
pub struct Theme {
    /// 整个窗口的画板底色
    pub bg: Hsla,
    /// 胶囊、右键卡片、空状态卡片的底色
    pub surface: Hsla,
    /// 浮层高光：胶囊里选中的滑块、按钮悬停底色
    pub surface_elevated: Hsla,
    /// 主要文字
    pub text: Hsla,
    /// 次要文字：未选中项、尺寸提示
    pub muted: Hsla,
    /// 弱提示文字：快捷键角标、辅助说明
    pub faint: Hsla,
    /// 强调色：主按钮、裁剪磁吸线、拉框选区
    pub accent: Hsla,
    /// 强调色上的文字
    pub accent_text: Hsla,
    /// 删除警示色
    pub danger: Hsla,
    /// 删除警示色上的文字
    pub danger_text: Hsla,
    /// 原地裁剪时遮住原图被裁掉部分的半透明遮罩
    pub ghost_mask: Hsla,
    /// 作品外框、胶囊、卡片的一像素细描边
    pub border_subtle: Hsla,
    /// 悬停底色
    pub hover: Hsla,
    /// 选中底色
    pub selected: Hsla,
    /// 滑条、开关的轨道
    pub track: Hsla,
}

fn color(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

impl Theme {
    /// 界面配色。全程序只有这一套深色配色。
    pub fn new() -> Self {
        Theme {
            bg: color(0x161618),
            surface: color(0x232326).opacity(0.92),
            surface_elevated: color(0x2c2c30),
            text: color(0xf3f2ee),
            muted: color(0x9a9890),
            faint: color(0x63615b),
            accent: color(0x3b82f6),
            accent_text: color(0xffffff),
            danger: color(0xf43f5e),
            danger_text: color(0xffffff),
            ghost_mask: color(0x161618).opacity(0.72),
            border_subtle: color(0xffffff).opacity(0.08),
            hover: color(0xffffff).opacity(0.06),
            selected: color(0xffffff).opacity(0.10),
            track: color(0xffffff).opacity(0.14),
        }
    }
}

/// 在两个界面颜色之间按 t（0 到 1）做线性插值。
pub fn lerp_hsla(a: Hsla, b: Hsla, t: f32) -> Hsla {
    Hsla {
        h: a.h + (b.h - a.h) * t,
        s: a.s + (b.s - a.s) * t,
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
