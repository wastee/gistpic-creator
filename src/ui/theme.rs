//! 界面配色。跟随系统浅色/深色外观。

use gpui::{Hsla, WindowAppearance};

#[derive(Clone, Copy)]
pub struct Theme {
    /// 窗口底色
    pub bg: Hsla,
    /// 面板、按钮底色
    pub panel: Hsla,
    /// 次级面板底色
    pub panel_alt: Hsla,
    /// 分隔线、边框
    pub border: Hsla,
    /// 主要文字
    pub text: Hsla,
    /// 次要文字
    pub muted: Hsla,
    /// 强调色（按钮、选中项）
    pub accent: Hsla,
    /// 强调色上的文字
    pub accent_text: Hsla,
    /// 悬停底色
    pub hover: Hsla,
    /// 滑条、开关的轨道
    pub track: Hsla,
    /// 错误提示
    pub danger: Hsla,
    /// 预览区背景
    pub preview_bg: Hsla,
    /// 预览里每张图的底板
    pub block_bg: Hsla,
    /// 弹窗遮罩
    pub overlay: Hsla,
}

fn color(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            _ => Self::light(),
        }
    }

    fn light() -> Self {
        Theme {
            bg: color(0xf3f3f5),
            panel: color(0xffffff),
            panel_alt: color(0xf7f7f9),
            border: color(0xd8d8de),
            text: color(0x1d1d1f),
            muted: color(0x6b6b73),
            accent: color(0x2f6fed),
            accent_text: color(0xffffff),
            hover: color(0xebebf0),
            track: color(0xdedee4),
            danger: color(0xd23b3b),
            preview_bg: color(0xe7e7ec),
            block_bg: color(0xffffff),
            overlay: color(0x000000),
        }
    }

    fn dark() -> Self {
        Theme {
            bg: color(0x1b1b1e),
            panel: color(0x26262a),
            panel_alt: color(0x212125),
            border: color(0x3a3a40),
            text: color(0xf0f0f4),
            muted: color(0x9c9ca6),
            accent: color(0x4f8cff),
            accent_text: color(0x0d0d0f),
            hover: color(0x32323a),
            track: color(0x3a3a42),
            danger: color(0xff6b6b),
            preview_bg: color(0x141416),
            block_bg: color(0x2a2a2f),
            overlay: color(0x000000),
        }
    }

    /// 弹窗遮罩用的半透明黑。
    pub fn overlay_color(&self) -> Hsla {
        self.overlay.opacity(0.55)
    }
}
