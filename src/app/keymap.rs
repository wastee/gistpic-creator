//! 快捷键绑定。单独放在这里，方便在测试里检查按键字符串能不能解析。

use gpui::KeyBinding;

use crate::platform;

use super::actions::*;

/// 全选这一条绑定的按键字符串，例如 `ctrl-a` 或 `cmd-a`。
///
/// 无头测试要按真实的平台按键走一遍，不能直接调动作函数：直接调就绕过了
/// 「按键绑定到底有没有注册上」这一环，而需求 7 报的正是快捷键不起作用。
pub fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

pub fn bindings() -> Vec<KeyBinding> {
    let key = platform::shortcut;
    let mut list = vec![
        KeyBinding::new(select_all_keystroke(), SelectAll, None),
        KeyBinding::new(&key("v"), PasteImage, None),
        KeyBinding::new(&key("o"), OpenImages, None),
        KeyBinding::new(&key("e"), ExportPng, None),
        KeyBinding::new(&key("shift-c"), CopyImage, None),
        KeyBinding::new(&key("z"), UndoEdit, None),
        KeyBinding::new(&key("shift-z"), RedoEdit, None),
        KeyBinding::new("delete", RemoveSelected, None),
        KeyBinding::new("backspace", RemoveSelected, None),
        KeyBinding::new(&key("0"), ZoomReset, None),
        KeyBinding::new(&key("+"), ZoomIn, None),
        KeyBinding::new(&key("="), ZoomIn, None),
        KeyBinding::new(&key("-"), ZoomOut, None),
        KeyBinding::new("escape", Dismiss, None),
    ];
    if cfg!(target_os = "macos") {
        list.push(KeyBinding::new("cmd-q", Quit, None));
    } else {
        list.push(KeyBinding::new("ctrl-q", Quit, None));
    }
    list
}
