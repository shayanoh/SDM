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

struct TrayState {
    mask: Vec<u8>,
    tracker: IconTracker,
    tint: Tint,
    last_tint_check: Instant,
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
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(image)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Shayan's Download Manager")
        .build(app)?;
    app.manage(Mutex::new(TrayState {
        mask,
        tracker,
        tint,
        last_tint_check: Instant::now(),
    }));
    Ok(())
}

/// Redraws the icon if `status` changes how it looks. Called every tick; a
/// tick that changes nothing visible costs a comparison.
pub fn refresh(app: &AppHandle, status: &TrayStatus) {
    let Some(state) = app.try_state::<Mutex<TrayState>>() else {
        return;
    };
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
}
