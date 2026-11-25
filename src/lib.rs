//! 省流图创造机（GistPic Creator）：字幕拼接与图片排版。

pub mod app;
pub mod config;
pub mod font_bootstrap;
pub mod i18n;
pub mod imaging;
pub mod model;
pub mod platform;
pub mod single_instance;
#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub mod startup;
pub mod tray;
pub mod ui;
pub mod vulkan_bootstrap;

/// 编进程序里的软件版本号，直接来自 Cargo.toml 的 version 字段，不手写。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 界面上显示的版本号文字，前面加 v，例如 v1.2.3。
pub fn version_label() -> String {
    format!("v{APP_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 界面显示的版本号必须跟包版本一致，否则用户看到的版本是错的。
    #[test]
    fn version_label_matches_package_version() {
        assert_eq!(version_label(), format!("v{}", env!("CARGO_PKG_VERSION")));
    }
}
