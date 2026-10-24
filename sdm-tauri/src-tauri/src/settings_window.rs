//! The Settings window: a second webview window loading the same frontend
//! bundle, which shows the Settings UI when its URL says `window=settings`.

use sdm_app::AppError;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

/// The width SwiftUI's Settings window has; the page sets the height to fit
/// the tab it shows.
const WIDTH: f64 = 560.0;
const INITIAL_HEIGHT: f64 = 440.0;

/// Shows the Settings window, creating it the first time. A second call
/// focuses the one that is open.
pub fn open(app: &AppHandle) -> Result<(), AppError> {
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
    .center()
    .build()
    .map(|_| ())
    .map_err(|e| AppError::failed(format!("could not open Settings: {e}")))
}

/// Tells every window to preview a theme (`None` restores the stored one).
pub fn broadcast_theme(app: &AppHandle, theme_id: Option<String>) -> Result<(), AppError> {
    app.emit("theme-preview", theme_id)
        .map_err(|e| AppError::failed(format!("could not announce the theme: {e}")))
}
