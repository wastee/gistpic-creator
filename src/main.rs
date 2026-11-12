//! 程序入口：创建窗口、注册快捷键。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::any::Any;
#[cfg(any(debug_assertions, feature = "dev-tools"))]
use std::time::Instant;

use gpui::prelude::*;
use gpui::{App, Application, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use gistpic_creator::app::AppRoot;
use gistpic_creator::app::keymap;
use gistpic_creator::config;
use gistpic_creator::font_bootstrap;
use gistpic_creator::i18n::Language;
use gistpic_creator::platform::APP_ID;
#[cfg(any(debug_assertions, feature = "dev-tools"))]
use gistpic_creator::startup;

fn main() {
    let default_filter = if cfg!(any(debug_assertions, feature = "dev-tools")) {
        "info"
    } else {
        "warn"
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_filter))
        .init();

    // GPUI 的窗口初始化依赖显卡（Linux 上是 Vulkan）。没有可用显卡时直接 panic，
    // 这里兜住，给用户一句能看懂的话，并返回非零退出码。
    let result = std::panic::catch_unwind(run);
    // 事件循环已经结束，但进程还活着。托盘的清理必须在这里做：
    // 拖到线程局部变量自己析构时，线程局部存储已经在销毁，异步驱动会报错。
    gistpic_creator::tray::shutdown();
    match result {
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
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    let started = Instant::now();
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    log::info!("开始初始化 GPUI");
    // 先精简 Linux 的字体扫描范围；必须在 GPUI 创建字体系统之前执行。
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    let font_started = Instant::now();
    let _font_bootstrap = font_bootstrap::apply();
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    startup::log_phase(font_started, "字体配置");
    let app = Application::new();
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    startup::log_phase(started, "GPUI 平台初始化");
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    let startup_started = started;
    app.run(move |cx: &mut App| {
        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        startup::log_phase(startup_started, "GPUI 事件循环启动");
        cx.bind_keys(keymap::bindings());

        // 没有保存记录或记录已经失效时，按最小尺寸在主屏居中打开，不最大化。
        let saved = config::load();
        let restored = saved.restore();
        let primary = cx
            .primary_display()
            .map(|display| config::WindowRect::from_bounds(display.bounds()))
            .unwrap_or(config::WindowRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            });
        let displays: Vec<config::WindowRect> = cx
            .displays()
            .iter()
            .map(|display| config::WindowRect::from_bounds(display.bounds()))
            .collect();
        let start = config::startup_window(restored.windowed, primary, &displays);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(start.to_bounds())),
                    titlebar: Some(TitlebarOptions {
                        title: Some(Language::default().app_name().into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    app_id: Some(APP_ID.to_string()),
                    window_min_size: Some(size(
                        px(config::MIN_WINDOW_WIDTH),
                        px(config::MIN_WINDOW_HEIGHT),
                    )),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| AppRoot::new(window, cx)),
            )
            .expect("创建窗口失败");
        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        startup::log_phase(startup_started, "主窗口创建完成");

        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        if startup::bench_enabled() {
            // 测量模式不启动托盘，避免 D-Bus 和托盘创建影响启动数字；
            // 等第一帧真正画完再退出，否则测到的是窗口对象创建时间。
            log::info!("启动测量模式：等第一帧画完");
            let _ = window.update(cx, |_root, window, _cx| {
                window.on_next_frame(move |_window, cx| {
                    startup::log_phase(startup_started, "第一帧完成");
                    cx.quit();
                });
            });
            return;
        }

        gistpic_creator::tray::start(cx, window);
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
