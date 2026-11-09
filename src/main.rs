//! 程序入口：创建窗口、注册快捷键。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::any::Any;

use gpui::prelude::*;
use gpui::{
    px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};

use video_screenshot::app::actions::Quit;
use video_screenshot::app::keymap;
use video_screenshot::app::AppRoot;
use video_screenshot::platform::{APP_ID, APP_TITLE};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // GPUI 的窗口初始化依赖显卡（Linux 上是 Vulkan）。没有可用显卡时直接 panic，
    // 这里兜住，给用户一句能看懂的话，并返回非零退出码。
    match std::panic::catch_unwind(run) {
        Ok(()) => {}
        Err(payload) => {
            eprintln!("启动失败：{}", panic_message(payload.as_ref()));
            eprintln!(
                "如果提示里包含 “Unable to init GPU context”，说明当前环境没有可用的显卡驱动。\
                 请确认系统支持 Vulkan（Linux 上可以运行 vulkaninfo 检查），并在有图形桌面的环境里启动本程序。"
            );
            std::process::exit(1);
        }
    }
}

fn run() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys(keymap::bindings());
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

        // 根据屏幕大小选一个合适的初始窗口尺寸，避免在小屏幕上超出屏幕。
        let (screen_width, screen_height) = cx
            .primary_display()
            .map(|display| {
                let bounds = display.bounds();
                (
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                )
            })
            .unwrap_or((1920.0, 1080.0));
        let window_width = (screen_width * 0.88).clamp(1240.0, 1360.0);
        let window_height = (screen_height * 0.86).clamp(720.0, 900.0);
        let bounds = Bounds::centered(None, size(px(window_width), px(window_height)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(APP_TITLE.into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    app_id: Some(APP_ID.to_string()),
                    window_min_size: Some(size(px(1240.), px(720.))),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| AppRoot::new(window, cx)),
            )
            .expect("创建窗口失败");
        video_screenshot::tray::start(cx, window);
        cx.activate(true);
    });
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "未知错误".to_string()
    }
}
