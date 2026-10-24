//! The tray popover window (macOS and Windows): a small frameless window
//! anchored to the tray icon, shown on a click and hidden when it loses focus.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sdm_app::AppError;
use sdm_app::tray::TrayStatus;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

pub const LABEL: &str = "tray";

/// The SwiftUI panel is 280 points wide; the page sets the height to fit.
const WIDTH: f64 = 280.0;
const INITIAL_HEIGHT: f64 = 300.0;
/// Space between the icon and the popover, in physical pixels.
const GAP: f64 = 4.0;
/// A click on the icon while the popover is open first takes its focus (and
/// hides it); the click then arrives as a toggle. Within this window after a
/// blur, the toggle is that same click and must not reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(250);

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the popover's top-left corner goes: centered on the icon, below it
/// when the icon is in the upper half of the work area (a menu bar) and above
/// it otherwise (a taskbar), kept inside the work area.
pub fn place(anchor: Area, size: (f64, f64), work: Area) -> (f64, f64) {
    let below = anchor.y + anchor.height / 2.0 < work.y + work.height / 2.0;
    let x = anchor.x + anchor.width / 2.0 - size.0 / 2.0;
    let y = if below {
        anchor.y + anchor.height + GAP
    } else {
        anchor.y - size.1 - GAP
    };
    (
        x.clamp(work.x, (work.x + work.width - size.0).max(work.x)),
        y.clamp(work.y, (work.y + work.height - size.1).max(work.y)),
    )
}

#[derive(Default)]
struct PopoverState {
    anchor: Option<Area>,
    last_status: Option<TrayStatus>,
    last_blur: Option<Instant>,
}

fn lock(state: &Mutex<PopoverState>) -> std::sync::MutexGuard<'_, PopoverState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers the popover's state. Call once at startup.
pub fn init(app: &AppHandle) {
    app.manage(Mutex::new(PopoverState::default()));
}

fn failed(what: &str, error: impl std::fmt::Display) -> AppError {
    AppError::failed(format!("{what}: {error}"))
}

fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?window=tray".into()))
            .title("SDM")
            .inner_size(WIDTH, INITIAL_HEIGHT)
            .decorations(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .map_err(|e| failed("could not create the tray popover", e))?;
    let handle = app.clone();
    let watched = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            lock(&handle.state::<Mutex<PopoverState>>()).last_blur = Some(Instant::now());
            let _ = watched.hide();
        }
        WindowEvent::Resized(_) if watched.is_visible().unwrap_or(false) => {
            // The page fits the window to its content after it shows: keep
            // it against the icon.
            let anchor = lock(&handle.state::<Mutex<PopoverState>>()).anchor;
            if let Some(anchor) = anchor {
                reposition(&watched, anchor);
            }
        }
        _ => {}
    });
    Ok(window)
}

fn reposition(window: &WebviewWindow, anchor: Area) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    let centre = (
        anchor.x + anchor.width / 2.0,
        anchor.y + anchor.height / 2.0,
    );
    let monitor = window
        .app_handle()
        .monitor_from_point(centre.0, centre.1)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let work = monitor.work_area();
    let work = Area {
        x: f64::from(work.position.x),
        y: f64::from(work.position.y),
        width: f64::from(work.size.width),
        height: f64::from(work.size.height),
    };
    let (x, y) = place(
        anchor,
        (f64::from(size.width), f64::from(size.height)),
        work,
    );
    if let Err(error) =
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
    {
        tracing::warn!("could not position the tray popover: {error}");
    }
}

/// A click on the tray icon: opens the popover against `anchor` (the icon's
/// rectangle) or closes it if it is open.
pub fn toggle(app: &AppHandle, anchor: Area) {
    let window = match ensure_window(app) {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!("{error}");
            return;
        }
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let status = {
        let shared = app.state::<Mutex<PopoverState>>();
        let mut state = lock(&shared);
        if state
            .last_blur
            .is_some_and(|at| at.elapsed() < REOPEN_GUARD)
        {
            return;
        }
        state.anchor = Some(anchor);
        state.last_status.clone()
    };
    reposition(&window, anchor);
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        tracing::warn!("could not show the tray popover: {error}");
        return;
    }
    if let Some(status) = status {
        let _ = app.emit_to(LABEL, "tray-status", status);
    }
}

/// Sends `status` to the popover when it changed and the popover is open;
/// remembers it either way so the next opening starts with it.
pub fn push_status(app: &AppHandle, status: &TrayStatus) {
    let Some(state) = app.try_state::<Mutex<PopoverState>>() else {
        return;
    };
    {
        let mut state = lock(&state);
        if state.last_status.as_ref() == Some(status) {
            return;
        }
        state.last_status = Some(status.clone());
    }
    let visible = app
        .get_webview_window(LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    if visible {
        let _ = app.emit_to(LABEL, "tray-status", status.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area {
        x: 0.0,
        y: 24.0,
        width: 1920.0,
        height: 1056.0,
    };
    const SIZE: (f64, f64) = (280.0, 300.0);

    fn icon_at(x: f64, y: f64) -> Area {
        Area {
            x,
            y,
            width: 24.0,
            height: 24.0,
        }
    }

    #[test]
    fn an_icon_in_the_top_bar_gets_the_popover_below_it_centered() {
        let (x, y) = place(icon_at(1500.0, 0.0), SIZE, WORK);
        assert_eq!((x, y), (1500.0 + 12.0 - 140.0, 24.0 + GAP));
    }

    #[test]
    fn an_icon_in_a_bottom_taskbar_gets_the_popover_above_it() {
        let work = Area {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let (x, y) = place(icon_at(1500.0, 1040.0), SIZE, work);
        assert_eq!((x, y), (1500.0 + 12.0 - 140.0, 1040.0 - 300.0 - GAP));
    }

    #[test]
    fn the_popover_never_leaves_the_work_area_sideways() {
        assert_eq!(place(icon_at(1910.0, 0.0), SIZE, WORK).0, 1920.0 - 280.0);
        assert_eq!(place(icon_at(0.0, 0.0), SIZE, WORK).0, 0.0);
    }

    #[test]
    fn a_popover_taller_than_the_room_sticks_to_the_work_area_edge() {
        let (_, y) = place(icon_at(1800.0, 0.0), (280.0, 5000.0), WORK);
        assert_eq!(y, WORK.y + WORK.height - 5000.0_f64.min(WORK.height));
    }
}
