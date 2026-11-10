//! 记住界面状态：输出参数、语言、窗口大小位置等，下次启动自动恢复。
//!
//! 存放位置是系统的用户配置目录下的 `gistpic-creator/state.json`。
//! 读写出任何问题都只打日志，不影响程序启动和退出。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::model::{GapSetting, LayoutTarget, Mode, Rgba, Settings};
use crate::ui::tokens;

/// 配置格式版本。以后改字段时加一，读到不认识的版本就用默认值。
const VERSION: u32 = 1;

/// 窗口的大小和位置。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 存到磁盘上的全部状态。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedState {
    pub version: u32,
    /// 界面语言，用 `Language::key()` 的值
    pub language: String,
    /// 当前模式，用 `Mode::key()` 的值
    pub mode: String,
    /// 是否裁剪字幕
    pub crop: bool,
    /// 是否保留第二行字幕
    pub keep_second: bool,
    /// 出血位比例
    pub bleed: f32,
    pub gap_enabled: bool,
    pub gap_px: u32,
    pub gap_color: [u8; 4],
    /// 排版目标，用 `LayoutTarget::key()` 的值
    pub layout: String,
    /// 预览缩放
    pub zoom: f32,
    /// 关闭窗口时是否隐藏到托盘
    pub close_to_tray: bool,
    /// 窗口大小和位置。没有值表示首次启动或位置失效。
    pub window: Option<WindowRect>,
}

impl Default for SavedState {
    fn default() -> Self {
        let settings = Settings::default();
        SavedState {
            version: VERSION,
            language: Language::default().key().to_string(),
            mode: Mode::default().key().to_string(),
            crop: settings.crop,
            keep_second: settings.keep_second,
            bleed: settings.bleed,
            gap_enabled: settings.gap.enabled,
            gap_px: settings.gap.px,
            gap_color: rgba_to_array(settings.gap.color),
            layout: settings.layout.key().to_string(),
            zoom: 1.0,
            close_to_tray: true,
            window: None,
        }
    }
}

impl SavedState {
    /// 从界面状态生成一份可保存的记录。
    pub fn capture(
        language: Language,
        mode: Mode,
        settings: &Settings,
        zoom: f32,
        close_to_tray: bool,
        window: Option<WindowRect>,
    ) -> Self {
        SavedState {
            version: VERSION,
            language: language.key().to_string(),
            mode: mode.key().to_string(),
            crop: settings.crop,
            keep_second: settings.keep_second,
            bleed: tokens::clamp_bleed(settings.bleed),
            gap_enabled: settings.gap.enabled,
            gap_px: tokens::clamp_gap_px(settings.gap.px),
            gap_color: rgba_to_array(settings.gap.color),
            layout: settings.layout.key().to_string(),
            zoom: tokens::clamp_zoom(zoom),
            close_to_tray,
            window,
        }
    }

    /// 把记录还原成各个状态。所有数值都夹到合法范围内。
    pub fn restore(&self) -> RestoredState {
        RestoredState {
            language: Language::ALL
                .into_iter()
                .find(|item| item.key() == self.language)
                .unwrap_or_default(),
            mode: match self.mode.as_str() {
                "layout" => Mode::Layout,
                _ => Mode::Subtitle,
            },
            settings: Settings {
                crop: self.crop,
                keep_second: self.keep_second,
                bleed: tokens::clamp_bleed(self.bleed),
                gap: GapSetting {
                    enabled: self.gap_enabled,
                    px: tokens::clamp_gap_px(self.gap_px),
                    color: array_to_rgba(self.gap_color),
                },
                layout: layout_from_key(&self.layout),
            },
            zoom: tokens::clamp_zoom(self.zoom),
            close_to_tray: self.close_to_tray,
            // 宽高不是正数的记录整个丢掉，交给调用方按屏幕大小重新算。
            // 这里必须先判断再修正，否则 0 会被补成最小尺寸，变成"有个有效位置"。
            window: self
                .window
                .filter(|rect| {
                    rect.width.is_finite()
                        && rect.height.is_finite()
                        && rect.width > 0.0
                        && rect.height > 0.0
                })
                .map(sanitize_window),
        }
    }
}

/// 还原出来的各项状态。
#[derive(Clone, Debug)]
pub struct RestoredState {
    pub language: Language,
    pub mode: Mode,
    pub settings: Settings,
    pub zoom: f32,
    pub close_to_tray: bool,
    pub window: Option<WindowRect>,
}

fn rgba_to_array(color: Rgba) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}

fn array_to_rgba(values: [u8; 4]) -> Rgba {
    Rgba {
        r: values[0],
        g: values[1],
        b: values[2],
        a: values[3],
    }
}

fn layout_from_key(key: &str) -> LayoutTarget {
    match key {
        "9-16" => LayoutTarget::Aspect(9.0 / 16.0),
        "4-3" => LayoutTarget::Aspect(4.0 / 3.0),
        "1-1" => LayoutTarget::Aspect(1.0),
        "row" => LayoutTarget::Row,
        "column" => LayoutTarget::Column,
        _ => LayoutTarget::Aspect(16.0 / 9.0),
    }
}

/// 把窗口尺寸夹到合法范围，防止手改配置文件后窗口小到看不见。
fn sanitize_window(rect: WindowRect) -> WindowRect {
    const MIN_WIDTH: f32 = 1240.0;
    const MIN_HEIGHT: f32 = 720.0;
    WindowRect {
        x: if rect.x.is_finite() { rect.x } else { 0.0 },
        y: if rect.y.is_finite() { rect.y } else { 0.0 },
        width: if rect.width.is_finite() {
            rect.width.max(MIN_WIDTH)
        } else {
            MIN_WIDTH
        },
        height: if rect.height.is_finite() {
            rect.height.max(MIN_HEIGHT)
        } else {
            MIN_HEIGHT
        },
    }
}

/// 配置文件路径。拿不到系统配置目录时返回 None。
pub fn state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("gistpic-creator").join("state.json"))
}

/// 读配置。文件不存在、读不了、内容坏了，都退回默认值。
pub fn load() -> SavedState {
    let Some(path) = state_path() else {
        log::warn!("拿不到配置目录，界面设置使用默认值");
        return SavedState::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return SavedState::default(),
        Err(err) => {
            log::warn!("读取界面设置失败（{}）：{err}", path.display());
            return SavedState::default();
        }
    };
    let state: SavedState = match serde_json::from_str(&text) {
        Ok(state) => state,
        Err(err) => {
            log::warn!("界面设置文件内容有问题，本次使用默认值（文件保留不覆盖）：{err}");
            return SavedState::default();
        }
    };
    if state.version != VERSION {
        log::warn!(
            "界面设置版本是 {}，本程序用版本 {}，本次使用默认值",
            state.version,
            VERSION
        );
        return SavedState::default();
    }
    state
}

/// 写配置。先写临时文件再改名，避免写到一半中断留下坏文件。
pub fn save(state: &SavedState) {
    let Some(path) = state_path() else {
        log::warn!("拿不到配置目录，界面设置没有保存");
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        log::warn!("创建配置目录失败（{}）：{err}", parent.display());
        return;
    }
    let text = match serde_json::to_string_pretty(state) {
        Ok(text) => text,
        Err(err) => {
            log::warn!("界面设置序列化失败：{err}");
            return;
        }
    };
    let temp = path.with_extension("json.tmp");
    if let Err(err) = std::fs::write(&temp, text) {
        log::warn!("写界面设置失败（{}）：{err}", temp.display());
        return;
    }
    if let Err(err) = std::fs::rename(&temp, &path) {
        log::warn!("替换界面设置文件失败（{}）：{err}", path.display());
        let _ = std::fs::remove_file(&temp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_matches_project_defaults() {
        let state = SavedState::default();
        let restored = state.restore();
        assert_eq!(restored.settings, Settings::default());
        assert_eq!(restored.language, Language::default());
        assert_eq!(restored.mode, Mode::default());
        assert_eq!(restored.zoom, 1.0);
    }

    /// 完整走一遍「当前状态 -> 存成 JSON -> 读回来 -> 恢复」，确认重启后界面状态真能还原。
    /// 前面的测试只验证了结构体能序列化，这里验证 capture 和 restore 两个方向都对得上。
    #[test]
    fn capture_then_restore_round_trips_every_setting() {
        let settings = Settings {
            crop: false,
            keep_second: true,
            bleed: 0.25,
            gap: crate::model::GapSetting {
                enabled: true,
                px: 18,
                color: Rgba {
                    r: 0x10,
                    g: 0x20,
                    b: 0x30,
                    a: 0xff,
                },
            },
            layout: LayoutTarget::Aspect(4.0 / 3.0),
        };

        let saved = SavedState::capture(
            Language::EnUs,
            Mode::Layout,
            &settings,
            1.75,
            false,
            Some(WindowRect {
                x: 120.0,
                y: 60.0,
                width: 1500.0,
                height: 950.0,
            }),
        );

        // 经过一次真实的 JSON 往返，模拟写盘再读盘。
        let text = serde_json::to_string(&saved).expect("序列化应成功");
        let parsed: SavedState = serde_json::from_str(&text).expect("反序列化应成功");
        let restored = parsed.restore();

        assert_eq!(restored.language, Language::EnUs, "语言应恢复");
        assert_eq!(restored.mode, Mode::Layout, "模式应恢复");
        assert_eq!(restored.zoom, 1.75, "缩放应恢复");
        assert!(!restored.close_to_tray, "关闭行为应恢复");

        let back = restored.settings;
        assert!(!back.crop, "裁剪字幕开关应恢复");
        assert!(back.keep_second, "保留第二行应恢复");
        assert_eq!(back.bleed, 0.25, "出血位应恢复");
        assert!(back.gap.enabled, "缝隙开关应恢复");
        assert_eq!(back.gap.px, 18, "缝隙宽度应恢复");
        assert_eq!(
            back.gap.color,
            Rgba {
                r: 0x10,
                g: 0x20,
                b: 0x30,
                a: 0xff,
            },
            "缝隙颜色应恢复"
        );
        assert_eq!(
            back.layout,
            LayoutTarget::Aspect(4.0 / 3.0),
            "排版目标应恢复"
        );

        let rect = restored.window.expect("窗口记录应恢复");
        assert_eq!(rect.x, 120.0);
        assert_eq!(rect.y, 60.0);
        assert_eq!(rect.width, 1500.0);
        assert_eq!(rect.height, 950.0);
    }

    #[test]
    fn state_round_trips_through_json() {
        let original = SavedState {
            language: "en-us".to_string(),
            mode: "layout".to_string(),
            crop: false,
            keep_second: true,
            bleed: 0.15,
            gap_enabled: true,
            gap_px: 12,
            gap_color: [255, 0, 0, 255],
            layout: "9-16".to_string(),
            zoom: 1.4,
            close_to_tray: false,
            window: Some(WindowRect {
                x: 100.0,
                y: 50.0,
                width: 1400.0,
                height: 900.0,
            }),
            ..Default::default()
        };

        let text = serde_json::to_string(&original).expect("序列化应成功");
        let back: SavedState = serde_json::from_str(&text).expect("反序列化应成功");
        assert_eq!(original, back);
    }

    #[test]
    fn restore_clamps_out_of_range_values() {
        let state = SavedState {
            zoom: 9.0,
            bleed: 5.0,
            gap_px: 999,
            ..Default::default()
        };
        let restored = state.restore();
        assert_eq!(restored.zoom, tokens::ZOOM_MAX);
        assert_eq!(restored.settings.bleed, tokens::BLEED_MAX);
        assert_eq!(restored.settings.gap.px, tokens::GAP_MAX);
    }

    #[test]
    fn restore_handles_unknown_keys() {
        let state = SavedState {
            layout: "不认识的写法".to_string(),
            mode: "不认识的模式".to_string(),
            language: "xx".to_string(),
            ..Default::default()
        };
        let restored = state.restore();
        assert_eq!(restored.settings.layout, LayoutTarget::default());
        assert_eq!(restored.mode, Mode::default());
        assert_eq!(restored.language, Language::default());
    }

    #[test]
    fn restore_rejects_tiny_or_broken_window() {
        // 高度是无穷大：不是有限值，整个窗口记录丢掉。
        let broken = SavedState {
            window: Some(WindowRect {
                x: f32::NAN,
                y: 0.0,
                width: 10.0,
                height: f32::INFINITY,
            }),
            ..Default::default()
        };
        assert!(
            broken.restore().window.is_none(),
            "含非法数值的窗口记录应整个丢掉"
        );

        // 数值都合法但太小：保留记录，尺寸补到最小值，坐标修掉 NaN。
        let tiny = SavedState {
            window: Some(WindowRect {
                x: f32::NAN,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
            ..Default::default()
        };
        let rect = tiny
            .restore()
            .window
            .expect("数值合法的窗口记录应保留并修正");
        assert_eq!(rect.x, 0.0, "NaN 坐标应被改成 0");
        assert_eq!(rect.width, 1240.0, "过小的宽应补到最小值");
        assert_eq!(rect.height, 720.0, "过小的高应补到最小值");

        // 宽高为 0：整个丢掉。
        let empty = SavedState {
            window: Some(WindowRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            ..Default::default()
        };
        assert!(empty.restore().window.is_none(), "宽高为 0 应整个丢掉");
    }

    #[test]
    fn every_layout_key_round_trips() {
        for target in LayoutTarget::PRESETS {
            let restored = layout_from_key(target.key());
            assert_eq!(restored, target, "{} 换回来要一样", target.key());
        }
    }

    #[test]
    fn every_language_key_round_trips() {
        for language in Language::ALL {
            let state = SavedState {
                language: language.key().to_string(),
                ..Default::default()
            };
            assert_eq!(state.restore().language, language);
        }
    }

    #[test]
    fn capture_writes_current_settings() {
        let settings = Settings {
            crop: false,
            bleed: 0.10,
            ..Default::default()
        };
        let state = SavedState::capture(Language::EnUs, Mode::Layout, &settings, 1.75, false, None);
        assert_eq!(state.language, "en-us");
        assert_eq!(state.mode, "layout");
        assert!(!state.crop);
        assert!((state.bleed - 0.10).abs() < 1e-5);
        assert!((state.zoom - 1.75).abs() < 1e-5);
        assert!(!state.close_to_tray);
    }

    /// 老版本写下来的配置文件里还有 `rail_pinned` 这类已删掉的字段，
    /// 新版本必须照常读出来，不能因为多了个字段就退回默认值把用户的设置全丢掉。
    #[test]
    fn old_config_files_with_removed_fields_still_load() {
        let old = r#"{
            "version": 1,
            "language": "en-us",
            "mode": "layout",
            "crop": false,
            "keep_second": true,
            "bleed": 0.2,
            "gap_enabled": true,
            "gap_px": 16,
            "gap_color": [255, 255, 255, 255],
            "layout": "1-1",
            "zoom": 1.5,
            "rail_pinned": true,
            "close_to_tray": false,
            "window": { "x": 10.0, "y": 20.0, "width": 1300.0, "height": 800.0 }
        }"#;
        let state: SavedState = serde_json::from_str(old).expect("旧配置文件应能读出来");
        let restored = state.restore();
        assert_eq!(restored.language, Language::EnUs);
        assert_eq!(restored.mode, Mode::Layout);
        assert_eq!(restored.zoom, 1.5);
        assert!(!restored.close_to_tray);
        assert_eq!(restored.settings.gap.px, 16);
        assert!(restored.settings.gap.enabled);
        assert_eq!(restored.settings.gap.color, Rgba::WHITE);
        assert_eq!(restored.settings.layout, LayoutTarget::Aspect(1.0));
        assert!(restored.window.is_some(), "窗口位置也要保住");
    }

    /// 新版本保存时不该再写回已删掉的字段，避免新旧字段长期并存。
    #[test]
    fn saved_state_no_longer_contains_removed_fields() {
        let text = serde_json::to_string(&SavedState::default()).expect("序列化应成功");
        assert!(
            !text.contains("rail_pinned"),
            "已删除的界面状态不该再写进配置文件"
        );
    }
}
