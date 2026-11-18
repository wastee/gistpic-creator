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
