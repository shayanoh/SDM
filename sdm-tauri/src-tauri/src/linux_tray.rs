//! Whether anything on this desktop can show a tray icon.

/// Windows and macOS always have a tray. On Linux an icon is only visible
/// when a StatusNotifier host is running (GNOME needs the AppIndicator
/// extension). Asked when the window closes, not only at startup, because the
/// extension can be switched on while SDM runs.
pub fn tray_host_present() -> bool {
    #[cfg(target_os = "linux")]
    {
        status_notifier_watcher_has_owner().unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg(target_os = "linux")]
fn status_notifier_watcher_has_owner() -> zbus::Result<bool> {
    let connection = zbus::blocking::Connection::session()?;
    let reply = connection.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &("org.kde.StatusNotifierWatcher",),
    )?;
    Ok(reply.body().deserialize::<bool>()?)
}
