//! Showing the main window from the tray, a notification or a second launch.

use std::sync::Mutex;

use sdm_app::AppError;
use tauri::{AppHandle, Emitter, Manager};

pub const LABEL: &str = "main";

/// Whether the page can hear a tab request yet. A request made while the
/// window is still loading (the app was started by the Chrome extension, or
/// by a login item) would be lost as an event, so it is parked until the page
/// asks for it with `take_pending_route`.
#[derive(Debug, Default)]
pub struct Navigation {
    page_ready: bool,
    pending: Option<String>,
    /// A request to show the window arrived before the page did.
    show_requested: bool,
}

impl Navigation {
    /// The route to send to the page now, or `None` when it was parked.
    pub fn request(&mut self, route: &str) -> Option<String> {
        if self.page_ready {
            Some(route.to_owned())
        } else {
            self.pending = Some(route.to_owned());
            None
        }
    }

    /// A request to show the main window: `true` when it must wait for the
    /// page (the window is created hidden and a page that has not painted
    /// would show white), in which case the page's `window_ready` shows it.
    pub fn defer_show(&mut self) -> bool {
        if self.page_ready {
            false
        } else {
            self.show_requested = true;
            true
        }
    }

    /// Whether a show request is waiting for the page; cleared by asking.
    pub fn take_show_requested(&mut self) -> bool {
        std::mem::take(&mut self.show_requested)
    }

    /// The page is listening: returns what was parked for it, once.
    pub fn page_ready(&mut self) -> Option<String> {
        self.page_ready = true;
        self.pending.take()
    }
}

pub type NavigationState = Mutex<Navigation>;

/// Shows, unminimizes and focuses the main window, puts it back in the
/// taskbar, and, if `route` names a tab (`downloads`, `completed` or
/// `linkgrabber`), tells the window to switch to it.
pub fn show(app: &AppHandle, route: Option<&str>) -> Result<(), AppError> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| AppError::failed("the main window is gone"))?;
    #[cfg(target_os = "macos")]
    crate::activation::apply(app, true);
    let deferred = app.try_state::<NavigationState>().is_some_and(|state| {
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .defer_show()
    });
    if deferred {
        window
            .set_skip_taskbar(false)
            .map_err(|e| AppError::failed(format!("could not show the main window: {e}")))?;
    } else {
        window
            .set_skip_taskbar(false)
            .and_then(|()| window.show())
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus())
            .map_err(|e| AppError::failed(format!("could not show the main window: {e}")))?;
    }
    if let Some(route) = route {
        let now = match app.try_state::<NavigationState>() {
            Some(state) => state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .request(route),
            None => Some(route.to_owned()),
        };
        if let Some(route) = now {
            app.emit_to(LABEL, "navigate", route)
                .map_err(|e| AppError::failed(format!("could not switch tabs: {e}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_before_the_page_is_ready_is_parked_and_handed_over_once() {
        let mut navigation = Navigation::default();
        assert_eq!(navigation.request("linkgrabber"), None);
        assert_eq!(navigation.page_ready().as_deref(), Some("linkgrabber"));
        assert_eq!(navigation.page_ready(), None);
    }

    #[test]
    fn a_show_request_before_the_page_waits_for_it_once() {
        let mut navigation = Navigation::default();
        assert!(navigation.defer_show());
        assert!(navigation.take_show_requested());
        assert!(!navigation.take_show_requested());
    }

    #[test]
    fn a_show_request_after_the_page_is_ready_is_not_deferred() {
        let mut navigation = Navigation::default();
        navigation.page_ready();
        assert!(!navigation.defer_show());
        assert!(!navigation.take_show_requested());
    }

    #[test]
    fn the_latest_parked_request_wins() {
        let mut navigation = Navigation::default();
        navigation.request("completed");
        navigation.request("linkgrabber");
        assert_eq!(navigation.page_ready().as_deref(), Some("linkgrabber"));
    }

    #[test]
    fn a_request_after_the_page_is_ready_goes_straight_through() {
        let mut navigation = Navigation::default();
        assert_eq!(navigation.page_ready(), None);
        assert_eq!(
            navigation.request("linkgrabber").as_deref(),
            Some("linkgrabber")
        );
        assert_eq!(navigation.page_ready(), None);
    }
}
