//! Linux 上判断当前桌面有没有托盘宿主。
//!
//! 托盘图标通过 StatusNotifierItem（D-Bus）显示，桌面必须有程序提供
//! `org.kde.StatusNotifierWatcher` 这个服务，图标才会出现在状态栏。
//! GNOME 默认没有，需要启用 AppIndicator 扩展。

/// 当前会话的托盘宿主是否存在。
pub(crate) fn tray_host_available() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &("org.kde.StatusNotifierWatcher",),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}
