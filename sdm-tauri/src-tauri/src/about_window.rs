//! The About window: a second webview window on the same bundle, shown when
//! its URL says `window=about`.

use sdm_app::AppError;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "about";

const WIDTH: f64 = 420.0;
const INITIAL_HEIGHT: f64 = 520.0;

/// Held while the window is looked up and built (two quick opens would
/// otherwise both find no window and make two).
static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Shows the About window, creating it the first time. A second call focuses
/// the one that is open.
pub fn open(app: &AppHandle) -> Result<(), AppError> {
    let _opening = OPENING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(window) = app.get_webview_window(LABEL) {
        window
            .show()
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus())
            .map_err(|e| AppError::failed(format!("could not show About: {e}")))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=about".into()),
    )
    .title("About SDM")
    .inner_size(WIDTH, INITIAL_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .visible(false)
    .center()
    .build()
    .map(|_| crate::window_ready::show_if_stuck(app, LABEL))
    .map_err(|e| AppError::failed(format!("could not open About: {e}")))
}

/// Closes the About window if it is open (with the main window).
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(error) = window.close()
    {
        tracing::warn!("could not close About: {error}");
    }
}
