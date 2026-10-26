//! Closing the main window and quitting: what each platform does, and the
//! guard that asks first when downloads are running.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sdm_app::App;
use sdm_app::tray::{QUIT_TITLE, TrayStatus};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
}

pub fn current_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide it and its taskbar button; the tray icon reaches it again.
    HideToTray,
    /// No tray to come back through: quit, asking first if downloads run.
    QuitViaGuard,
    /// macOS: hide only; the Dock icon or the menu bar icon reopens it.
    HideWindow,
}

pub fn close_action(platform: Platform, tray_present: bool) -> CloseAction {
    match (platform, tray_present) {
        (Platform::MacOs, _) => CloseAction::HideWindow,
        (_, true) => CloseAction::HideToTray,
        (_, false) => CloseAction::QuitViaGuard,
    }
}

/// What the quit warning dialog says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardPrompt {
    pub title: &'static str,
    pub message: String,
}

/// `None` when nothing runs and the app may quit at once.
pub fn guard_prompt(status: &TrayStatus) -> Option<GuardPrompt> {
    status.quit_warning().map(|message| GuardPrompt {
        title: QUIT_TITLE,
        message,
    })
}

/// Set once a quit is approved, so the exit it triggers is let through.
static QUITTING: AtomicBool = AtomicBool::new(false);
/// Set while the warning is on screen, so a second request does not stack one.
static ASKING: AtomicBool = AtomicBool::new(false);

fn quit_now(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Quits, after asking first if any download is running.
pub fn request_quit(app: &AppHandle) {
    let status = app.state::<Arc<App>>().tray_status();
    let Some(prompt) = guard_prompt(&status) else {
        quit_now(app);
        return;
    };
    if ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    app.dialog()
        .message(prompt.message)
        .title(prompt.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".to_owned(),
            "Cancel".to_owned(),
        ))
        .show(move |confirmed| {
            ASKING.store(false, Ordering::SeqCst);
            if confirmed {
                quit_now(&handle);
            }
        });
}

/// The main window's close button.
pub fn on_close_requested(app: &AppHandle) {
    match close_action(current_platform(), crate::linux_tray::tray_host_present()) {
        CloseAction::HideToTray => hide_main(app, true),
        CloseAction::HideWindow => hide_main(app, false),
        CloseAction::QuitViaGuard => request_quit(app),
    }
}

fn hide_main(app: &AppHandle, leave_taskbar: bool) {
    if let Some(window) = app.get_webview_window(crate::main_window::LABEL) {
        if leave_taskbar && let Err(error) = window.set_skip_taskbar(true) {
            tracing::warn!("could not remove the taskbar button: {error}");
        }
        if let Err(error) = window.hide() {
            tracing::warn!("could not hide the main window: {error}");
        }
    }
    crate::settings_window::close(app);
    #[cfg(target_os = "macos")]
    crate::activation::apply(app, false);
}

/// The run loop's events: every exit goes through the guard unless approved,
/// and a Dock click reopens the window on macOS.
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        RunEvent::ExitRequested { api, .. } if !QUITTING.load(Ordering::SeqCst) => {
            api.prevent_exit();
            request_quit(app);
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if let Err(error) = crate::main_window::show(app, None) {
                tracing::warn!("could not reopen the main window: {error}");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_app::tray::TrayStatus;

    #[test]
    fn closing_hides_to_the_tray_except_where_there_is_no_tray_and_on_the_mac() {
        use CloseAction::*;
        use Platform::*;
        assert_eq!(close_action(Windows, true), HideToTray);
        assert_eq!(close_action(Linux, true), HideToTray);
        assert_eq!(close_action(Linux, false), QuitViaGuard);
        assert_eq!(close_action(MacOs, true), HideWindow);
        assert_eq!(close_action(MacOs, false), HideWindow);
    }

    fn status(running: u32, non_resumable: u32) -> TrayStatus {
        TrayStatus {
            running_count: running,
            non_resumable_count: non_resumable,
            bytes_per_second: 0.0,
            history: vec![],
            overall_fraction: 0.0,
            items: vec![],
            links_waiting: 0,
        }
    }

    #[test]
    fn with_nothing_running_the_guard_lets_the_app_quit_at_once() {
        assert_eq!(guard_prompt(&status(0, 0)), None);
    }

    #[test]
    fn with_downloads_running_the_guard_asks_with_the_warning_text() {
        let prompt = guard_prompt(&status(2, 1)).expect("a prompt");
        assert_eq!(prompt.title, "Quit with active downloads in progress?");
        assert!(prompt.message.starts_with("2 downloads are running."));
        assert!(prompt.message.contains("1 of them cannot be resumed."));
    }
}
