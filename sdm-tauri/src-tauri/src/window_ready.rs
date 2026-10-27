//! Windows are created hidden and shown by their page once its theme is
//! painted and its size fitted (`window_ready`); until then a window would
//! show a white, unstyled page, and a slow machine makes that gap long.

use std::time::Duration;

use sdm_app::AppError;
use tauri::{AppHandle, Manager, WebviewWindow};

/// If a page never reports in (a script error), show its window anyway. A
/// healthy page reports well inside this; reaching it is logged as a warning.
const FALLBACK: Duration = Duration::from_secs(2);

/// Whether the page's "ready" shows its window. The tray popover shows itself
/// on a click, and a main window started hidden in the tray stays hidden
/// unless something already asked for it (`requested`).
pub fn should_reveal(label: &str, hidden_launch: bool, requested: bool) -> bool {
    match label {
        // `popover::LABEL`, which only exists on macOS and Windows.
        "tray" => false,
        crate::main_window::LABEL => !hidden_launch || requested,
        _ => true,
    }
}

fn hidden_launch() -> bool {
    !cfg!(target_os = "macos")
        && crate::autostart::is_hidden_launch(&std::env::args().collect::<Vec<_>>())
}

/// Called by a page when it has painted: shows (and focuses) its window.
pub fn reveal(window: &WebviewWindow) -> Result<(), AppError> {
    let requested = window.label() == crate::main_window::LABEL
        && window
            .app_handle()
            .try_state::<crate::main_window::NavigationState>()
            .is_some_and(|state| {
                state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take_show_requested()
            });
    if !should_reveal(window.label(), hidden_launch(), requested) {
        return Ok(());
    }
    window
        .show()
        .and_then(|()| window.set_focus())
        .map_err(|e| AppError::failed(format!("could not show the window: {e}")))
}

/// Shows window `label` after a while if its page has not done so itself.
pub fn show_if_stuck(app: &AppHandle, label: &'static str) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FALLBACK).await;
        if let Some(window) = app.get_webview_window(label)
            && !window.is_visible().unwrap_or(true)
            && should_reveal(label, hidden_launch(), false)
        {
            tracing::warn!("{label} never reported ready; showing it anyway");
            if let Err(error) = window.show() {
                tracing::warn!("could not show {label}: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_shows_its_window_except_the_popover_and_a_hidden_start() {
        assert!(should_reveal("settings", false, false));
        assert!(should_reveal("chrome-setup", false, false));
        assert!(should_reveal("main", false, false));
        assert!(!should_reveal("main", true, false));
        assert!(should_reveal("main", true, true), "something asked for it");
        assert!(should_reveal("settings", true, false));
        assert!(!should_reveal("tray", false, true));
    }
}
