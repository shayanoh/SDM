//! The Settings window: a second webview window loading the same frontend
//! bundle, which shows the Settings UI when its URL says `window=settings`.

use sdm_app::AppError;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

/// The width SwiftUI's Settings window has; the page sets the height to fit
/// the tab it shows.
const WIDTH: f64 = 560.0;
const INITIAL_HEIGHT: f64 = 440.0;

/// Held while the window is looked up and built. Commands run on separate
/// threads and Tauri only registers a window after it is built, so without it
/// two quick opens (a double-clicked gear, or the menu accelerator and the
/// page's own shortcut together) would both find no window and make two.
static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Shows the Settings window, creating it the first time. A second call
/// focuses the one that is open.
pub fn open(app: &AppHandle) -> Result<(), AppError> {
    let _opening = OPENING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(window) = app.get_webview_window(LABEL) {
        window
            .show()
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus())
            .map_err(|e| AppError::failed(format!("could not show Settings: {e}")))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=settings".into()),
    )
    .title("Settings")
    .inner_size(WIDTH, INITIAL_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .visible(false)
    .center()
    .build()
    .map(|_| crate::window_ready::show_if_stuck(app, LABEL))
    .map_err(|e| AppError::failed(format!("could not open Settings: {e}")))
}

/// Tells every window to preview a theme (`None` restores the stored one).
pub fn broadcast_theme(app: &AppHandle, theme_id: Option<String>) -> Result<(), AppError> {
    app.emit("theme-preview", theme_id)
        .map_err(|e| AppError::failed(format!("could not announce the theme: {e}")))
}

/// Closes the Settings window if it is open. The app has no other reason to
/// run once the main window is gone, and Settings would be all that is left.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(error) = window.close()
    {
        tracing::warn!("could not close Settings: {error}");
    }
}
