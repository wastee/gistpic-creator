//! 平台相关的小工具：窗口标识、修饰键、默认保存目录、时间戳。

use std::path::PathBuf;

/// Linux 下的 app id（任务栏图标分组用）。
pub const APP_ID: &str = "com.gistpic.creator";

/// 当前平台的主修饰键：macOS 用 cmd，其余用 ctrl。
pub fn command_modifier() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// 拼一个带主修饰键的按键绑定字符串，例如 `ctrl-v`。
pub fn shortcut(key: &str) -> String {
    format!("{}-{}", command_modifier(), key)
}

/// 界面上显示的快捷键文案，例如 `Ctrl+V`。
pub fn shortcut_label(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("Cmd+{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

/// 导出图片的默认目录：优先“图片”，其次“下载”，最后用户主目录。
pub fn default_save_dir() -> PathBuf {
    fn existing(path: Option<PathBuf>) -> Option<PathBuf> {
        path.filter(|path| path.is_dir())
    }

    existing(dirs::picture_dir())
        .or_else(|| existing(dirs::download_dir()))
        .or_else(|| existing(dirs::home_dir()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 本地时间戳，格式 `20260101_083000`，用于导出文件名。
pub fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}
