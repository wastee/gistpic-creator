//! 系统托盘：Windows、macOS、Linux 都显示一个托盘图标。
//!
//! 左键点图标（Windows / macOS）或菜单里的「打开主窗口」把窗口叫到前面；
//! 右键菜单有「打开主窗口」和「退出」。
//!
//! Linux 用 ksni 后端（StatusNotifierItem，走 D-Bus），不需要 GTK 或 libappindicator，
//! 但需要桌面提供托盘宿主；GNOME 需要启用 AppIndicator 扩展。

mod icon;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use std::cell::{Cell, RefCell};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, WindowHandle};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app::AppRoot;
use crate::i18n::Language;

/// 托盘上能触发的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayAction {
    OpenWindow,
    Quit,
}

#[derive(Debug, thiserror::Error)]
enum TrayError {
    #[error("创建托盘菜单失败：{0}")]
    Menu(#[from] tray_icon::menu::Error),
    #[error("托盘图标数据不合法：{0}")]
    Icon(#[from] tray_icon::BadIcon),
    #[error("创建托盘图标失败：{0}")]
    Build(#[from] tray_icon::Error),
}

impl TrayError {
    fn localized(&self, language: Language) -> String {
        match language {
            Language::ZhCn => self.to_string(),
            Language::EnUs => match self {
                TrayError::Menu(err) => format!("Failed to create tray menu: {err}"),
                TrayError::Icon(err) => format!("Invalid tray icon data: {err}"),
                TrayError::Build(err) => format!("Failed to create tray icon: {err}"),
            },
        }
    }
}

/// 托盘图标和菜单项句柄。托盘库的类型不能在多个线程间发送，这里放在主线程局部变量里。
struct TrayUi {
    tray: TrayIcon,
    open_item: MenuItem,
    quit_item: MenuItem,
}

thread_local! {
    static TRAY_UI: RefCell<Option<TrayUi>> = const { RefCell::new(None) };
    static TRAY_LANGUAGE: Cell<Language> = const { Cell::new(Language::ZhCn) };
}

/// 启动托盘。必须在主线程调用（由 `main` 在窗口创建后调用）。
pub fn start(app: &mut App, window: WindowHandle<AppRoot>) {
    let handle = window;

    // 等到窗口画出第一帧再创建托盘：macOS 要求创建托盘时事件循环已经在运行，
    // Windows 也要求创建托盘的线程上已经有消息循环。
    let result = window.update(app, |root, window, _cx| {
        TRAY_LANGUAGE.with(|current| current.set(root.language));
        window.on_next_frame(move |_window, cx| {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::info!("第一帧完成，开始创建托盘");
            spawn_event_task(cx, handle);
        });
    });
    if let Err(error) = result {
        log::warn!("注册托盘启动回调失败：{error}");
    }
}

/// 退出前主动清理托盘。
///
/// 必须由 `main` 在 GPUI 事件循环结束后调用，不能留给线程局部变量自己析构。
/// 原因：`TRAY_UI` 是在主线程的线程局部存储里，进程退出时 Rust 运行时才开始析构它，
/// 此时线程局部存储已经进入「正在销毁」状态。托盘的析构函数里要 join 监听线程、
/// 等 D-Bus 关闭，中间会阻塞等待异步任务，而异步驱动自己也要访问线程局部存储。
/// 于是就会报 “cannot access a Thread Local Storage value during or after destruction”。
///
/// 在这里主动取走托盘的句柄并析构，此时进程完全正常，异步驱动、线程局部存储都还在。
/// 清理失败不影响退出，只打日志。
pub fn shutdown() {
    TRAY_UI.with(|slot| {
        let taken = slot.borrow_mut().take();
        drop(taken);
    });
}

/// 界面语言切换时更新托盘菜单文字和悬停提示。
pub fn set_language(language: Language) {
    TRAY_LANGUAGE.with(|current| current.set(language));
    TRAY_UI.with(|slot| {
        if let Some(ui) = slot.borrow().as_ref() {
            ui.open_item.set_text(language.tray_open_window());
            ui.quit_item.set_text(language.tray_quit());
            let _ = ui.tray.set_tooltip(Some(language.app_name()));
        }
    });
}

fn spawn_event_task(cx: &mut App, window: WindowHandle<AppRoot>) {
    let (sender, mut receiver) = unbounded::<TrayAction>();
    cx.spawn(async move |cx| {
        let language = TRAY_LANGUAGE.with(Cell::get);
        if let Err(error) = install(sender, language) {
            log::error!("{error}");
            let message = error.localized(language);
            let _ = window.update(cx, |root, _window, cx| {
                root.set_status(message, true, cx);
            });
            return;
        }
        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        log::info!("系统托盘图标已创建");

        #[cfg(target_os = "linux")]
        if !linux::tray_host_available() {
            log::warn!("当前桌面没有托盘宿主（org.kde.StatusNotifierWatcher 不存在）");
            let _ = window.update(cx, |root, _window, cx| {
                root.set_status(root.language.tray_no_host(), false, cx);
            });
        }

        while let Some(action) = receiver.next().await {
            match action {
                TrayAction::OpenWindow => open_window(cx, &window),
                TrayAction::Quit => {
                    let _ = cx.update(|cx| cx.quit());
                }
            }
        }
    })
    .detach();
}

/// 把窗口叫到前面；窗口被隐藏时重新显示，最小化时尽量恢复。
fn open_window(cx: &mut gpui::AsyncApp, window: &WindowHandle<AppRoot>) {
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    log::info!("托盘打开主窗口");
    let _ = window.update(cx, |_root, window, _cx| {
        // 窗口可能被关闭按钮隐藏了，先重新映射，再请求激活。
        window.set_visible(true);
        window.activate_window();
    });
    let _ = cx.update(|cx| cx.activate(true));
    #[cfg(target_os = "macos")]
    macos::restore_windows();
}

/// 创建图标和菜单，并注册点击/菜单事件；出错时返回可读的错误。
fn install(sender: UnboundedSender<TrayAction>, language: Language) -> Result<(), TrayError> {
    let menu = Menu::new();
    let open_item = MenuItem::new(language.tray_open_window(), true, None);
    let quit_item = MenuItem::new(language.tray_quit(), true, None);
    let open_id = open_item.id().clone();
    let quit_id = quit_item.id().clone();
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let (rgba, width, height) = icon::tray_rgba(cfg!(target_os = "macos"));
    let icon = Icon::from_rgba(rgba, width, height)?;

    let builder = TrayIconBuilder::new()
        .with_id("gistpic-creator-tray")
        .with_tooltip(language.app_name())
        .with_menu(Box::new(menu))
        // Windows / macOS 上左键点图标直接打开窗口，右键才弹菜单。
        // Linux 后端由桌面决定点击行为，这个设置会被忽略。
        .with_menu_on_left_click(false);
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_templated(icon);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.with_icon(icon);

    let tray = builder.build()?;

    let click_sender = sender.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = click_sender.unbounded_send(TrayAction::OpenWindow);
        }
    }));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            let _ = sender.unbounded_send(TrayAction::OpenWindow);
        } else if event.id == quit_id {
            let _ = sender.unbounded_send(TrayAction::Quit);
        }
    }));

    // 托盘句柄必须一直活着，图标和菜单才会保留；程序退出时由系统回收。
    TRAY_UI.with(|slot| {
        *slot.borrow_mut() = Some(TrayUi {
            tray: tray.clone(),
            open_item: open_item.clone(),
            quit_item: quit_item.clone(),
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_is_safe_when_tray_never_started() {
        // 托盘创建前就退出（比如显卡初始化失败）时，也必须能安全调用。
        // 不 panic 就算通过。
        shutdown();
        shutdown();
    }

    /// 真正创建托盘，再在「进程还活着」的时候清理掉。
    ///
    /// 这是修复的核心：不主动清理的话，托盘的析构会拖到线程局部变量析构阶段，
    /// 那时异步驱动已经不能访问线程局部存储了。
    /// 这个测试要连 D-Bus，没有会话总线时直接跳过。
    #[test]
    fn creating_and_shutting_down_tray_does_not_panic() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("没有 D-Bus 会话总线，跳过");
            return;
        }
        let (sender, _receiver) = unbounded::<TrayAction>();
        match install(sender, Language::ZhCn) {
            Ok(()) => {}
            Err(error) => {
                // 桌面没有托盘宿主时不报错，只是没有图标可用。
                eprintln!("创建托盘失败（可接受）：{error}");
                return;
            }
        }
        // 主动清理：模拟程序退出时 main 做的事。
        shutdown();
    }
}

