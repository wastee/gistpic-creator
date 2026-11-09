//! macOS 专用：GPUI 的窗口激活只做 `makeKeyAndOrderFront`，不会取消最小化，
//! 这里直接用 AppKit 把所有最小化的窗口恢复出来。

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

/// 恢复应用里所有最小化的窗口，并把窗口带到前面。
pub(crate) fn restore_windows() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
    }
}
