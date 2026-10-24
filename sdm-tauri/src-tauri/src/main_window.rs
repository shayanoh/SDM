//! Showing the main window from the tray, a notification or a second launch.

use sdm_app::AppError;
use tauri::{AppHandle, Emitter, Manager};

pub const LABEL: &str = "main";

/// Shows, unminimizes and focuses the main window, puts it back in the
/// taskbar, and, if `route` names a tab (`downloads`, `completed` or
/// `linkgrabber`), tells the window to switch to it.
pub fn show(app: &AppHandle, route: Option<&str>) -> Result<(), AppError> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| AppError::failed("the main window is gone"))?;
    window
        .set_skip_taskbar(false)
        .and_then(|()| window.show())
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus())
        .map_err(|e| AppError::failed(format!("could not show the main window: {e}")))?;
    if let Some(route) = route {
        app.emit_to(LABEL, "navigate", route)
            .map_err(|e| AppError::failed(format!("could not switch tabs: {e}")))?;
    }
    Ok(())
}
