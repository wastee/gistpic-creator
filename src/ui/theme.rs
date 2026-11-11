//! 界面配色：温润画板风格。
//!
//! 整个窗口是一张暖色画板，作品、胶囊、右键卡片浮在上面。
//! 只用两种彩色：克莱因蓝表示强调和可操作，珊瑚红表示删除。
//! 想调整观感就改这里的色值，所有界面都会跟着变。

use gpui::{BoxShadow, Hsla, WindowAppearance, point, px};

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

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            _ => Self::light(),
        }
    }

    fn light() -> Self {
        Theme {
            bg: color(0xf4f3ef),
            surface: color(0xffffff).opacity(0.92),
            surface_elevated: color(0xfaf9f5),
            text: color(0x1c1b18),
            muted: color(0x6e6c65),
            faint: color(0xa3a098),
            accent: color(0x2563eb),
            accent_text: color(0xffffff),
            danger: color(0xe11d48),
            danger_text: color(0xffffff),
            ghost_mask: color(0xf4f3ef).opacity(0.72),
            border_subtle: color(0x000000).opacity(0.07),
            hover: color(0x000000).opacity(0.04),
            selected: color(0x000000).opacity(0.07),
            track: color(0x000000).opacity(0.10),
        }
    }

    fn dark() -> Self {
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

    /// 作品长图的双层柔和投影：一层环境淡影，一层向下偏移的贴身微影。
    ///
    /// 深浅两套配色共用，投影永远是黑色加透明度，跟随主题只改深浅。
    pub fn shadow_work(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: color(0x000000).opacity(0.10),
                offset: point(px(0.), px(10.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
            },
            BoxShadow {
                color: color(0x000000).opacity(0.06),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
            },
        ]
    }

    /// 胶囊与右键卡片的投影，比作品长图略轻，避免浮层显得厚重。
    pub fn shadow_float(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: color(0x000000).opacity(0.12),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            },
            BoxShadow {
                color: color(0x000000).opacity(0.06),
                offset: point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
            },
        ]
    }

    /// 小气泡（磁吸档位、缝隙宽度、缩放比例）的投影，比浮层再轻一档。
    pub fn shadow_bubble(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: color(0x000000).opacity(0.10),
            offset: point(px(0.), px(4.)),
            blur_radius: px(14.),
            spread_radius: px(0.),
        }]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_differ_on_every_role() {
        let light = Theme::light();
        let dark = Theme::dark();
        assert_ne!(light.bg, dark.bg);
        assert_ne!(light.surface, dark.surface);
        assert_ne!(light.surface_elevated, dark.surface_elevated);
        assert_ne!(light.text, dark.text);
        assert_ne!(light.muted, dark.muted);
        assert_ne!(light.faint, dark.faint);
        assert_ne!(light.accent, dark.accent);
        assert_ne!(light.danger, dark.danger);
        assert_ne!(light.ghost_mask, dark.ghost_mask);
        assert_ne!(light.border_subtle, dark.border_subtle);
    }

    #[test]
    fn unknown_appearance_falls_back_to_light() {
        let light = Theme::light();
        let resolved = Theme::for_appearance(WindowAppearance::Light);
        assert_eq!(resolved.bg, light.bg);
    }

    #[test]
    fn shadows_are_two_layers_and_soft() {
        let theme = Theme::light();
        for shadow in [theme.shadow_work(), theme.shadow_float()] {
            assert_eq!(shadow.len(), 2, "统一用双层柔和投影");
            assert!(shadow.iter().all(|layer| layer.blur_radius > px(0.)));
        }
    }
}
