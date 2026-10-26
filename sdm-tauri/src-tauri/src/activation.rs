//! macOS only: the Dock / Menu Bar mode (spec section 10.2 of the SwiftUI
//! design). Windows and Linux ignore the stored mode: their tray icon always
//! shows.

use std::sync::Arc;

use sdm_app::App;
use sdm_app::protocol::ActivationMode;
use tauri::{ActivationPolicy, AppHandle, Manager};

/// What the mode asks for right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub dock_icon: bool,
    pub tray_visible: bool,
}

/// `window_open` is whether the main window is showing.
pub fn plan(mode: ActivationMode, window_open: bool) -> Plan {
    match mode {
        ActivationMode::MenuBarOnly => Plan {
            dock_icon: window_open,
            tray_visible: true,
        },
        ActivationMode::DockOnly => Plan {
            dock_icon: true,
            tray_visible: false,
        },
        ActivationMode::Both => Plan {
            dock_icon: true,
            tray_visible: true,
        },
    }
}

/// Applies the stored mode, given whether the main window is (about to be)
/// showing. Safe to call at any time, including with no window open.
pub fn apply(app: &AppHandle, window_open: bool) {
    let mode = app.state::<Arc<App>>().get_settings().activation_mode;
    let plan = plan(mode, window_open);
    let policy = if plan.dock_icon {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    if let Err(error) = app.set_activation_policy(policy) {
        tracing::warn!("could not change the Dock icon: {error}");
    }
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID)
        && let Err(error) = tray.set_visible(plan.tray_visible)
    {
        tracing::warn!("could not show or hide the menu bar icon: {error}");
    }
}

/// [`apply`] for whatever the main window is doing now.
pub fn apply_current(app: &AppHandle) {
    let open = app
        .get_webview_window(crate::main_window::LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    apply(app, open);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_app::protocol::ActivationMode::*;

    fn plan_for(mode: sdm_app::protocol::ActivationMode, window: bool) -> (bool, bool) {
        let plan = plan(mode, window);
        (plan.dock_icon, plan.tray_visible)
    }

    #[test]
    fn menu_bar_only_has_a_dock_icon_only_while_a_window_is_open() {
        assert_eq!(plan_for(MenuBarOnly, true), (true, true));
        assert_eq!(plan_for(MenuBarOnly, false), (false, true));
    }

    #[test]
    fn dock_only_always_has_the_dock_icon_and_never_the_tray() {
        assert_eq!(plan_for(DockOnly, true), (true, false));
        assert_eq!(plan_for(DockOnly, false), (true, false));
    }

    #[test]
    fn both_always_has_both() {
        assert_eq!(plan_for(Both, true), (true, true));
        assert_eq!(plan_for(Both, false), (true, true));
    }
}
