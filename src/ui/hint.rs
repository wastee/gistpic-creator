//! 悬停说明：只画图标的按钮，指针压上去时在这个元素正上方补一句话。
//!
//! 需求 3 把底栏的「添加 / 复制 / 导出 / 横向排列 / 竖向排列」都改成了纯图标，
//! 文字说明收在这里。需求 5 之后气泡贴在各自按钮上，不再统一浮在胶囊居中位置：
//! 贴着按钮就不会看错说的是哪一个。
//!
//! 悬停本身是即时状态（见 `ui::hover`），这里只负责把说明画出来。

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, AnyElement, IntoElement, SharedString, div, px};

use crate::app::AppRoot;
use crate::i18n::Language;
use crate::ui::hover::HoverKey;
use crate::ui::theme::Theme;
use crate::ui::tokens::{self, radius, text_size};

/// 说明气泡的入场时长。
pub const HINT_ENTER_MS: u64 = 120;
/// 气泡底边与它依附的元素顶边之间留的缝隙。
pub const HINT_GAP: gpui::Pixels = px(8.);

/// 一个部件对应的说明：给内部名字，返回 (元素 id, 说明文字)。
///
/// 内部名字用来拼 `hover-hint-{名字}` 这个调试选择器，测试据此判断
/// 指针压在哪个按钮上时会说哪句话。
pub(crate) fn hover_hint_for(
    key: &HoverKey,
    language: Language,
) -> Option<(&'static str, &'static str)> {
    let HoverKey::Widget(name) = key else {
        return None;
    };
    let hint = match name.as_str() {
        "capsule-add" => ("capsule-add", language.add_images_hint()),
        "capsule-copy" => ("capsule-copy", language.copy_hint()),
        "export-png" => ("export-png", language.export_png_hint()),
        "capsule-undo" => ("capsule-undo", language.undo_hint()),
        "layout-row" => ("layout-row", language.layout_row_hint()),
        "layout-column" => ("layout-column", language.layout_column_hint()),
        "layout-shuffle" => ("layout-shuffle", language.layout_shuffle_hint()),
        // 右下角缩放胶囊：读数和两项操作各有一句话，气泡贴在胶囊上。
        "zoom-reading" => ("zoom-reading", language.zoom_percent_hint()),
        "zoom-fit" => ("zoom-fit", language.zoom_fit_hint()),
        "zoom-one-to-one" => ("zoom-one-to-one", language.zoom_one_to_one_hint()),
        _ => return None,
    };
    Some(hint)
}

/// 一个说明气泡。
fn hint_bubble(name: &'static str, text: &'static str, theme: Theme) -> AnyElement {
    let bubble = div()
        .debug_selector(move || format!("hover-hint-{name}"))
        .id(SharedString::from(format!("hover-hint-{name}")))
        .px(px(10.))
        .py(px(5.))
        .rounded(radius::PILL)
        .bg(theme.surface)
        .font_family(tokens::FONT_FAMILY)
        .text_size(text_size::CAPTION)
        .text_color(theme.text)
        // 说明是一句话，别在空格处折行。
        .whitespace_nowrap()
        .child(text);
    bubble
        .with_animation(
            SharedString::from(format!("hover-hint-enter-{name}")),
            Animation::new(std::time::Duration::from_millis(HINT_ENTER_MS)),
            |el, delta| el.opacity(delta),
        )
        .into_any_element()
}

impl AppRoot {
    /// 夹在某个元素内部的说明气泡：绝对定位在该元素正上方、右对齐。
    ///
    /// 元素本身要带 `relative()`，气泡才以它为基准摆放。`bottom` 是气泡底边
    /// 离元素底边的高度，要大于元素自身高度气泡才不会盖住它。只在这个部件被
    /// 悬停时才画出来，树里平时没有这个节点。
    pub(crate) fn render_attached_hint(
        &self,
        key: &HoverKey,
        bottom: gpui::Pixels,
        theme: Theme,
    ) -> Option<AnyElement> {
        if !self.hover_gate.is_active(key) {
            return None;
        }
        let (name, text) = hover_hint_for(key, self.language)?;
        Some(
            div()
                .absolute()
                .right(px(0.))
                .bottom(bottom)
                .child(hint_bubble(name, text, theme))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_only_button_has_a_hint() {
        for name in [
            "capsule-add",
            "capsule-copy",
            "export-png",
            "capsule-undo",
            "layout-row",
            "layout-column",
            "layout-shuffle",
        ] {
            let key = HoverKey::widget(name);
            for language in Language::ALL {
                let hint = hover_hint_for(&key, language);
                assert!(hint.is_some(), "{name} 应该有悬停说明（{language:?}）");
                let (id, text) = hint.unwrap();
                assert_eq!(id, name);
                assert!(!text.is_empty(), "{name} 的悬停说明不能是空话");
            }
        }
    }

    /// 画布图块、比例预设这些不需要说明的部件不能凭空冒出气泡。
    #[test]
    fn plain_targets_have_no_hint() {
        assert!(hover_hint_for(&HoverKey::widget("layout-preset-0"), Language::ZhCn).is_none());
        assert!(hover_hint_for(&HoverKey::widget("zoom-chip"), Language::ZhCn).is_none());
        assert!(hover_hint_for(&HoverKey::Item(1), Language::ZhCn).is_none());
    }

    /// 缩放胶囊的三处说明：百分比读数、适合画布、1:1，中英文都不能空。
    #[test]
    fn zoom_chip_parts_explain_themselves() {
        for name in ["zoom-reading", "zoom-fit", "zoom-one-to-one"] {
            let key = HoverKey::widget(name);
            for language in Language::ALL {
                let (id, text) = hover_hint_for(&key, language)
                    .unwrap_or_else(|| panic!("{name} 应该有悬停说明（{language:?}）"));
                assert_eq!(id, name);
                assert!(!text.is_empty());
            }
        }
    }
}
