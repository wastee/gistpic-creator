//! 快捷键绑定。单独放在这里，方便在测试里检查按键字符串能不能解析。

use gpui::KeyBinding;

use crate::platform;

use super::actions::*;

pub fn bindings() -> Vec<KeyBinding> {
    let key = platform::shortcut;
    let mut list = vec![
        KeyBinding::new(&key("v"), PasteImage, None),
        KeyBinding::new(&key("o"), OpenImages, None),
        KeyBinding::new(&key("e"), ExportPng, None),
        KeyBinding::new(&key("shift-c"), CopyImage, None),
        KeyBinding::new(&key("z"), UndoEdit, None),
        KeyBinding::new(&key("shift-z"), RedoEdit, None),
        KeyBinding::new("escape", DialogSkip, None),
        KeyBinding::new("delete", RemoveSelected, None),
        KeyBinding::new("backspace", RemoveSelected, None),
        KeyBinding::new(&key("0"), ZoomReset, None),
        KeyBinding::new(&key("+"), ZoomIn, None),
        KeyBinding::new(&key("="), ZoomIn, None),
        KeyBinding::new(&key("-"), ZoomOut, None),
    ];
    if cfg!(target_os = "macos") {
        list.push(KeyBinding::new("cmd-q", Quit, None));
    } else {
        list.push(KeyBinding::new("ctrl-q", Quit, None));
    }
    list
}
