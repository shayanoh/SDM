//! Launch at login: the OS's own state is the truth, the setting follows it.

use std::sync::Arc;

use sdm_app::{App, AppError};
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// The argument the login entry passes on Windows and Linux, so SDM starts in
/// the tray instead of opening its window.
pub const HIDDEN_FLAG: &str = "--hidden";

/// Whether this process was started by the login entry.
pub fn is_hidden_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == HIDDEN_FLAG)
}

/// The value to apply to the OS when the toggle goes from `previous` to
/// `wanted`; `None` when nothing changed.
pub fn login_change(previous: bool, wanted: bool) -> Option<bool> {
    (previous != wanted).then_some(wanted)
}

/// The value to store at startup when the OS disagrees with the stored one:
/// the OS is the truth (the user may have removed the entry by hand).
pub fn reconcile(stored: bool, os: bool) -> Option<bool> {
    (stored != os).then_some(os)
}

/// The plugin, registering the login entry with `--hidden` where SDM starts
/// in the tray (macOS follows the SwiftUI app and opens normally).
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let mut builder = tauri_plugin_autostart::Builder::new();
    if !cfg!(target_os = "macos") {
        builder = builder.arg(HIDDEN_FLAG);
    }
    builder.build()
}

fn failed(what: &str, error: impl std::fmt::Display) -> AppError {
    AppError::failed(format!("{what}: {error}"))
}

/// Adds or removes the login entry.
pub fn apply(app: &AppHandle, enabled: bool) -> Result<(), AppError> {
    let launcher = app.autolaunch();
    if enabled {
        launcher
            .enable()
            .map_err(|e| failed("could not turn on launch at login", e))
    } else {
        launcher
            .disable()
            .map_err(|e| failed("could not turn off launch at login", e))
    }
}

/// Whether the OS has the login entry now.
pub fn is_enabled(app: &AppHandle) -> Result<bool, AppError> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| failed("could not read launch at login", e))
}

/// At startup, makes the stored setting say what the OS says.
pub async fn reconcile_at_startup(app: &AppHandle, core: &Arc<App>) {
    let Ok(os) = is_enabled(app) else {
        return;
    };
    let mut settings = core.get_settings();
    if let Some(value) = reconcile(settings.launch_at_login, os) {
        settings.launch_at_login = value;
        if let Err(error) = core.update_settings(settings).await {
            tracing::warn!("could not record the launch-at-login state: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn a_launch_from_the_login_entry_is_hidden() {
        assert!(is_hidden_launch(&args(&["sdm", "--hidden"])));
        assert!(!is_hidden_launch(&args(&["sdm"])));
        assert!(!is_hidden_launch(&args(&["sdm", "--simulate", "20", "3"])));
    }

    #[test]
    fn only_a_changed_toggle_touches_the_os() {
        assert_eq!(login_change(false, true), Some(true));
        assert_eq!(login_change(true, false), Some(false));
        assert_eq!(login_change(true, true), None);
        assert_eq!(login_change(false, false), None);
    }

    #[test]
    fn at_startup_the_stored_value_follows_the_os() {
        assert_eq!(reconcile(false, true), Some(true));
        assert_eq!(reconcile(true, false), Some(false));
        assert_eq!(reconcile(true, true), None);
    }
}
