//! 界面配色。只用灰阶分层，只有强调色一个彩色。
//!
//! 界面里不用边框线区分区域，靠两层底色的深浅差。想调整的话改这里的色值，
//! 所有界面都会跟着变。

use gpui::{Hsla, WindowAppearance};

#[derive(Clone, Copy)]
pub struct Theme {
    /// 窗口底色、图标条底色
    pub bg: Hsla,
    /// 面板、浮层、菜单的底色
    pub surface: Hsla,
    /// 预览区背景，比窗口底色更深一档
    pub surface_sunken: Hsla,
    /// 主要文字
    pub text: Hsla,
    /// 次要文字：说明、标签
    pub muted: Hsla,
    /// 极淡文字：禁用态
    pub faint: Hsla,
    /// 悬停底色
    pub hover: Hsla,
    /// 选中底色
    pub selected: Hsla,
    /// 强调色。界面上只有导出按钮和拖拽落点用它
    pub accent: Hsla,
    /// 强调色上的文字
    pub accent_text: Hsla,
    /// 错误提示
    pub danger: Hsla,
    /// 滑条、开关的轨道
    pub track: Hsla,
    /// 浮层、菜单的阴影
    pub shadow: Hsla,
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
            bg: color(0xf7f7f8),
            surface: color(0xffffff),
            surface_sunken: color(0xefeff1),
            text: color(0x18181b),
            muted: color(0x71717a),
            faint: color(0xa1a1aa),
            hover: color(0x000000).opacity(0.04),
            selected: color(0x000000).opacity(0.07),
            accent: color(0x2563eb),
            accent_text: color(0xffffff),
            danger: color(0xdc2626),
            track: color(0xe4e4e7),
            shadow: color(0x000000),
        }
    }

    fn dark() -> Self {
        Theme {
            bg: color(0x141416),
            surface: color(0x1c1c1f),
            surface_sunken: color(0x0e0e10),
            text: color(0xf2f2f4),
            muted: color(0x8e8e96),
            faint: color(0x5c5c63),
            hover: color(0xffffff).opacity(0.06),
            selected: color(0xffffff).opacity(0.10),
            accent: color(0x3b82f6),
            accent_text: color(0xffffff),
            danger: color(0xf87171),
            track: color(0x2a2a2e),
            shadow: color(0x000000),
        }
    }

    /// 弹窗遮罩用的半透明黑。
    pub fn scrim(&self) -> Hsla {
        color(0x000000).opacity(0.55)
    }

    /// 浮层、菜单的阴影颜色。
    pub fn shadow_color(&self) -> Hsla {
        self.shadow.opacity(0.12)
    }

    /// 框选时选区外面那一层遮罩。
    pub fn dim(&self) -> Hsla {
        color(0x000000).opacity(0.55)
    }
}
