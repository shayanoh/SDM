//! The tray icon: installed once, redrawn when its picture would change.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sdm_app::tray::{IconTracker, TrayStatus};
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::tray_icon::{Tint, compose, downscale, system_tint};

/// The id of the one tray icon.
pub const TRAY_ID: &str = "main";

/// Side of the square icon we draw, in pixels: 16 points at 2x on macOS, and
/// a size the Windows and Linux shells scale down cleanly.
const ICON_SIZE: u32 = 32;

/// How often the system's dark/light setting is looked at again.
const RETINT_EVERY: Duration = Duration::from_secs(30);

/// The logo at icon size, as white RGBA whose alpha is the silhouette.
fn icon_alpha_mask() -> Vec<u8> {
    let logo = Image::from_bytes(include_bytes!("../icons/tray-logo.png"))
        .expect("the embedded tray logo is a valid PNG");
    downscale(
        logo.rgba(),
        logo.width(),
        logo.height(),
        ICON_SIZE,
        ICON_SIZE,
    )
}

/// One entry of the Linux tray menu.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    Separator,
}

/// The Linux tray menu for `status`: the actions, a separator, then the
/// disabled status lines (what the popover shows on the other platforms).
#[cfg(any(all(unix, not(target_os = "macos")), test))]
pub fn menu_spec(status: &TrayStatus) -> Vec<MenuEntry> {
    let item = |id: &str, label: String, enabled: bool| MenuEntry::Item {
        id: id.to_owned(),
        label,
        enabled,
    };
    let mut entries = vec![
        item("open", "Open SDM".to_owned(), true),
        item("pause", "Pause all".to_owned(), true),
    ];
    if status.links_waiting > 0 {
        let noun = if status.links_waiting == 1 {
            "link"
        } else {
            "links"
        };
        entries.push(item(
            "review",
            format!("Review ({} {noun} waiting)", status.links_waiting),
            true,
        ));
    }
    entries.push(item("quit", "Quit".to_owned(), true));
    entries.push(MenuEntry::Separator);
    for (index, line) in status.menu_status_lines().into_iter().enumerate() {
        entries.push(item(&format!("status-{index}"), line, false));
    }
    entries
}

/// When the icon was last double-clicked. The click that ends a double-click
/// arrives after it and must not reopen the popover just closed.
#[cfg(any(windows, target_os = "macos"))]
static LAST_DOUBLE_CLICK: Mutex<Option<Instant>> = Mutex::new(None);

#[cfg(any(windows, target_os = "macos"))]
const DOUBLE_CLICK_SETTLE: Duration = Duration::from_millis(600);

#[cfg(any(windows, target_os = "macos"))]
fn on_tray_icon_event(tray: &tauri::tray::TrayIcon, event: tauri::tray::TrayIconEvent) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
    if let TrayIconEvent::DoubleClick {
        button: MouseButton::Left,
        ..
    } = event
    {
        *LAST_DOUBLE_CLICK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        crate::popover::hide(tray.app_handle());
        log_failure(crate::main_window::show(tray.app_handle(), None));
        return;
    }
    let TrayIconEvent::Click {
        button_state: MouseButtonState::Up,
        rect,
        ..
    } = event
    else {
        return;
    };
    let settling = LAST_DOUBLE_CLICK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|at| at.elapsed() < DOUBLE_CLICK_SETTLE);
    if settling {
        return;
    }
    let scale = tray
        .app_handle()
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |monitor| monitor.scale_factor());
    let position = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    crate::popover::toggle(
        tray.app_handle(),
        crate::popover::Area {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        },
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
fn build_menu(app: &AppHandle, spec: &[MenuEntry]) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::new(app)?;
    for entry in spec {
        match entry {
            MenuEntry::Item { id, label, enabled } => {
                menu.append(&MenuItem::with_id(app, id, label, *enabled, None::<&str>)?)?
            }
            MenuEntry::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
        }
    }
    Ok(menu)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "open" => log_failure(crate::main_window::show(app, None)),
        "review" => log_failure(crate::main_window::show(app, Some("linkgrabber"))),
        "quit" => crate::lifecycle::request_quit(app),
        "pause" => {
            let core = std::sync::Arc::clone(&*app.state::<std::sync::Arc<sdm_app::App>>());
            tauri::async_runtime::spawn(async move {
                let _ = core
                    .guarded(|core| async move { core.pause_all().await })
                    .await;
            });
        }
        _ => {}
    }
}

fn log_failure(result: Result<(), sdm_app::AppError>) {
    if let Err(error) = result {
        tracing::warn!("tray menu action failed: {error}");
    }
}

struct TrayState {
    mask: Vec<u8>,
    tracker: IconTracker,
    tint: Tint,
    last_tint_check: Instant,
    #[cfg(all(unix, not(target_os = "macos")))]
    last_menu: Vec<MenuEntry>,
}

fn lock(state: &Mutex<TrayState>) -> std::sync::MutexGuard<'_, TrayState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Builds the tray icon in its idle state.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let tint = system_tint();
    let mask = icon_alpha_mask();
    let image = Image::new_owned(
        compose(&mask, ICON_SIZE, ICON_SIZE, false, 0.0, tint),
        ICON_SIZE,
        ICON_SIZE,
    );
    let mut tracker = IconTracker::new();
    tracker.needs_redraw(false, 0.0, ICON_SIZE);
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(image)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Shayan's Download Manager");
    // Windows and macOS: a click opens the popover. (Linux emits no click
    // events; it has the menu below.)
    #[cfg(any(windows, target_os = "macos"))]
    let builder = builder.on_tray_icon_event(on_tray_icon_event);
    #[cfg(all(unix, not(target_os = "macos")))]
    let initial_menu = menu_spec(&TrayStatus::idle());
    #[cfg(all(unix, not(target_os = "macos")))]
    let builder = builder
        .menu(&build_menu(app, &initial_menu)?)
        .on_menu_event(on_menu_event);
    builder.build(app)?;
    app.manage(Mutex::new(TrayState {
        mask,
        tracker,
        tint,
        last_tint_check: Instant::now(),
        #[cfg(all(unix, not(target_os = "macos")))]
        last_menu: initial_menu,
    }));
    Ok(())
}

/// Rebuilds the Linux menu when its entries differ from the last ones set
/// (a menu cannot change while open, so it is replaced, at most once a tick).
#[cfg(all(unix, not(target_os = "macos")))]
fn refresh_menu(app: &AppHandle, state: &mut TrayState, status: &TrayStatus) {
    let spec = menu_spec(status);
    if spec == state.last_menu {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app, &spec) {
        Ok(menu) => {
            if let Err(error) = tray.set_menu(Some(menu)) {
                tracing::warn!("could not update the tray menu: {error}");
            } else {
                state.last_menu = spec;
            }
        }
        Err(error) => tracing::warn!("could not build the tray menu: {error}"),
    }
}

/// Redraws the icon if `status` changes how it looks. Called every tick; a
/// tick that changes nothing visible costs a comparison.
pub fn refresh(app: &AppHandle, status: &TrayStatus) {
    let Some(state) = app.try_state::<Mutex<TrayState>>() else {
        return;
    };
    #[cfg(any(windows, target_os = "macos"))]
    crate::popover::push_status(app, status);
    let mut state = lock(&state);
    if state.last_tint_check.elapsed() >= RETINT_EVERY {
        state.last_tint_check = Instant::now();
        let tint = system_tint();
        if tint != state.tint {
            state.tint = tint;
            // A new color is a new picture even at the same progress.
            state.tracker = IconTracker::new();
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    refresh_menu(app, &mut state, status);
    let running = status.running_count > 0;
    if !state
        .tracker
        .needs_redraw(running, status.overall_fraction, ICON_SIZE)
    {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let rgba = compose(
        &state.mask,
        ICON_SIZE,
        ICON_SIZE,
        running,
        status.overall_fraction,
        state.tint,
    );
    if let Err(error) = tray.set_icon(Some(Image::new_owned(rgba, ICON_SIZE, ICON_SIZE))) {
        tracing::warn!("could not redraw the tray icon: {error}");
    }
    #[cfg(target_os = "macos")]
    let _ = tray.set_icon_as_template(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_logo_decodes_and_survives_downscaling() {
        let logo = icon_alpha_mask();
        assert_eq!(logo.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        let opaque = logo.chunks(4).filter(|px| px[3] > 200).count();
        assert!(
            opaque > 20,
            "the logo has solid pixels at icon size, got {opaque}"
        );
        let transparent = logo.chunks(4).filter(|px| px[3] < 20).count();
        assert!(
            transparent > 20,
            "and a transparent background, got {transparent}"
        );
    }

    fn status(running: u32, links_waiting: u32) -> TrayStatus {
        TrayStatus {
            running_count: running,
            non_resumable_count: 0,
            bytes_per_second: 1024.0 * 1024.0,
            history: vec![],
            overall_fraction: 0.0,
            items: (0..running)
                .map(|i| sdm_app::tray::TrayItem {
                    name: format!("f{i}.bin"),
                    fraction: 0.5,
                    bytes_per_second: 1024.0,
                })
                .collect(),
            links_waiting,
        }
    }

    fn labels(spec: &[MenuEntry]) -> Vec<String> {
        spec.iter()
            .map(|entry| match entry {
                MenuEntry::Item { label, .. } => label.clone(),
                MenuEntry::Separator => "---".to_owned(),
            })
            .collect()
    }

    #[test]
    fn an_idle_menu_has_the_actions_a_separator_and_one_status_line() {
        let spec = menu_spec(&status(0, 0));
        assert_eq!(
            labels(&spec),
            vec![
                "Open SDM",
                "Pause all",
                "Quit",
                "---",
                "No downloads running"
            ]
        );
        let enabled: Vec<bool> = spec
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item { enabled, .. } => Some(*enabled),
                MenuEntry::Separator => None,
            })
            .collect();
        assert_eq!(enabled, vec![true, true, true, false]);
    }

    #[test]
    fn waiting_links_add_a_review_entry_before_quit() {
        assert_eq!(
            labels(&menu_spec(&status(0, 4)))[..4],
            ["Open SDM", "Pause all", "Review (4 links waiting)", "Quit"]
        );
        assert_eq!(
            labels(&menu_spec(&status(0, 1)))[2],
            "Review (1 link waiting)"
        );
    }

    #[test]
    fn running_downloads_list_five_lines_then_and_n_more_all_disabled() {
        let spec = menu_spec(&status(7, 0));
        let after_separator: Vec<_> = spec
            .iter()
            .skip_while(|e| !matches!(e, MenuEntry::Separator))
            .skip(1)
            .collect();
        assert_eq!(after_separator.len(), 1 + 5 + 1);
        assert!(
            after_separator
                .iter()
                .all(|e| matches!(e, MenuEntry::Item { enabled: false, .. }))
        );
        assert_eq!(labels(&spec).last().unwrap(), "and 2 more");
    }
}
