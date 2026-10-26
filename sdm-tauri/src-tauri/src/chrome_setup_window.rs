//! The Chrome extension setup window: a second webview window on the same
//! bundle, shown when its URL says `window=chrome-setup`.

use sdm_app::AppError;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "chrome-setup";

const WIDTH: f64 = 560.0;
const INITIAL_HEIGHT: f64 = 640.0;

/// Held while the window is looked up and built (two quick opens would
/// otherwise both find no window and make two).
static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Shows the setup window, creating it the first time.
pub fn open(app: &AppHandle) -> Result<(), AppError> {
    let _opening = OPENING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(window) = app.get_webview_window(LABEL) {
        window
            .show()
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus())
            .map_err(|e| AppError::failed(format!("could not show the setup window: {e}")))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=chrome-setup".into()),
    )
    .title("Install the Chrome Extension")
    .inner_size(WIDTH, INITIAL_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .always_on_top(true)
    .center()
    .build()
    .map(|_| ())
    .map_err(|e| AppError::failed(format!("could not open the setup window: {e}")))
}

/// Closes the setup window if it is open (with the main window).
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(error) = window.close()
    {
        tracing::warn!("could not close the setup window: {error}");
    }
}
