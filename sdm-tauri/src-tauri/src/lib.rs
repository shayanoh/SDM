mod clipboard;
mod commands;
mod host;
mod lifecycle;
mod linux_tray;
mod main_window;
mod notify;
mod settings_window;
mod tray;
mod tray_icon;
mod vibrancy;

use std::sync::Arc;
use std::time::Duration;

use commands::*;
use sdm_app::host::Host;
use sdm_app::simulation::{SimulationHost, populate, simulated_dependencies};
use sdm_app::{App, AppError};
use sdm_core::AppTiming;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};

/// The id of the app menu's "Settings…" item on macOS.
#[cfg(target_os = "macos")]
const SETTINGS_MENU_ID: &str = "settings";

/// macOS: the default app menu with "Settings…" (Cmd+,) added under About, as
/// the SwiftUI app has it. Windows and Linux get no menu bar.
#[cfg(target_os = "macos")]
fn install_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(application)) = menu.items()?.into_iter().next() {
        let settings = MenuItem::with_id(
            app,
            SETTINGS_MENU_ID,
            "Settings…",
            true,
            Some("CmdOrCtrl+,"),
        )?;
        application.insert(&settings, 2)?;
        application.insert(&PredefinedMenuItem::separator(app)?, 3)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        attach,
        add_url,
        start_items,
        stop_items,
        retry_items,
        retry_mux_items,
        reset_items,
        set_items_enabled,
        remove_items,
        remove_package,
        move_items,
        move_items_to_new_package,
        reorder_items,
        reorder_packages,
        describe_deletion,
        open_item,
        reveal_item,
        reveal_package,
        pause_all,
        resume_all,
        set_list_visible,
        set_detail_items,
        full_state,
        get_settings,
        update_settings,
        binaries_status,
        check_binaries_now,
        list_themes,
        open_settings,
        show_main,
        broadcast_theme,
        theme_css,
        vibrancy_active,
        grabber_ingest_text,
        grabber_remove_links,
        grabber_remove_package,
        grabber_clear,
        grabber_move_link,
        grabber_rename_package,
        grabber_merge_packages,
        grabber_split_package,
        grabber_recheck,
        grabber_recheck_failed,
        grabber_cancel_checks,
        grabber_set_format,
        grabber_copy_url,
        grabber_add_to_downloads,
    ])
}

/// Writes `web/src/bindings.ts`. Run by a debug build at startup and by
/// `cargo test -p sdm-tauri export_bindings`.
#[cfg(debug_assertions)]
fn export_bindings(builder: &Builder<tauri::Wry>) {
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../web/src/bindings.ts",
        )
        .expect("failed to export the TypeScript bindings");
}

/// `Some((items, active))` when launched with `--simulate <items> <active>`.
/// Only debug builds and builds with the `simulate` feature honor the flag.
fn simulation_request() -> Result<Option<(usize, usize)>, AppError> {
    #[cfg(any(debug_assertions, feature = "simulate"))]
    {
        let args: Vec<String> = std::env::args().collect();
        sdm_app::simulation::parse_simulate_args(&args)
    }
    #[cfg(not(any(debug_assertions, feature = "simulate")))]
    {
        Ok(None)
    }
}

type Built = (Arc<App>, Option<(usize, usize)>);

/// The production app, or the load harness (on a scratch folder under the
/// system temp directory, never the real data directory) when requested.
fn build_app(handle: &tauri::AppHandle) -> Result<Built, Box<dyn std::error::Error>> {
    if let Some((items, active)) = simulation_request()? {
        let root = std::env::temp_dir().join(format!("sdm-simulation-{}", std::process::id()));
        tracing::warn!(
            "simulating {items} items, {active} active, in {}",
            root.display()
        );
        let host: Arc<dyn Host> = Arc::new(SimulationHost::new(root));
        let dependencies = simulated_dependencies(host.as_ref(), items, active);
        let app = App::new_with_concurrency(host, dependencies, active)?;
        return Ok((app, Some((items, active))));
    }
    let host = Arc::new(host::TauriHost::new(handle)?);
    Ok((App::production(host)?, None))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        // Remembers the main window's size, position and maximized state.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&[settings_window::LABEL])
                .build(),
        )
        .invoke_handler(builder.invoke_handler())
        .on_window_event(|window, event| {
            // Settings is a second window, so closing the main one no longer
            // ends the app by itself: take Settings down with it.
            if window.label() != main_window::LABEL {
                return;
            }
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Closing is a request, answered by the platform's rule.
                    api.prevent_close();
                    lifecycle::on_close_requested(window.app_handle());
                }
                tauri::WindowEvent::Destroyed => settings_window::close(window.app_handle()),
                _ => {}
            }
        })
        .on_menu_event(|app, event| {
            #[cfg(target_os = "macos")]
            if event.id() == SETTINGS_MENU_ID {
                // A window made inside a menu event handler can deadlock on
                // some platforms; its own task is safe everywhere.
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = settings_window::open(&handle) {
                        tracing::error!("could not open Settings: {error}");
                    }
                });
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        })
        .setup(move |tauri_app| {
            builder.mount_events(tauri_app);
            notify::install_click_handler(tauri_app.handle())?;
            #[cfg(target_os = "macos")]
            install_menu(tauri_app.handle())?;
            let (app, simulation) = build_app(tauri_app.handle())?;
            tauri_app.manage(Arc::clone(&app));
            clipboard::spawn(tauri_app.handle().clone(), Arc::clone(&app));
            tray::install(tauri_app.handle())?;
            let effect = tauri_app
                .get_webview_window("main")
                .and_then(|window| vibrancy::apply(&window));
            tauri_app.manage(vibrancy::VibrancyState(effect));

            // Restore, then provision binaries in the background, then beat.
            let heartbeat_app = Arc::clone(&app);
            let tray_handle = tauri_app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some((items, active)) = simulation {
                    if let Err(error) = populate(&heartbeat_app, items, active).await {
                        tracing::error!("could not load the simulation: {error}");
                    }
                    // The idle measurement: everything loaded, nothing running.
                    if std::env::var_os("SDM_SIMULATE_IDLE").is_some()
                        && let Err(error) = heartbeat_app.pause_all().await
                    {
                        tracing::error!("could not pause the simulation: {error}");
                    }
                } else {
                    if let Err(error) = heartbeat_app.restore().await {
                        tracing::error!("could not restore saved downloads: {error}");
                    }
                    let provisioner = Arc::clone(&heartbeat_app);
                    tauri::async_runtime::spawn(
                        async move { provisioner.provision_binaries().await },
                    );
                }

                let period = Duration::from_millis(1000 / AppTiming::TICKS_PER_SECOND as u64);
                let mut interval = tokio::time::interval(period);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    // A panic in a tick must not end the heartbeat.
                    let app = Arc::clone(&heartbeat_app);
                    let _ = app
                        .guarded(|app| async move {
                            app.heartbeat().await;
                            Ok(())
                        })
                        .await;
                    tray::refresh(&tray_handle, &heartbeat_app.tray_status());
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the SDM application")
        .run(|app, event| lifecycle::on_run_event(app, &event));
}

#[cfg(all(test, debug_assertions))]
mod tests {
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }

    /// The main window is configured like the SwiftUI one: its title, its
    /// default and minimum sizes, and no webview file drop (which on Windows
    /// would swallow the HTML5 drag and drop the list relies on).
    #[test]
    fn the_main_window_is_configured_like_the_swiftui_one() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid config");
        let window = &config["app"]["windows"][0];
        assert_eq!(window["label"], "main");
        assert_eq!(window["title"], "Shayan's Download Manager");
        assert_eq!(window["width"], 1000);
        assert_eq!(window["height"], 640);
        assert_eq!(window["minWidth"], 760);
        assert_eq!(window["minHeight"], 480);
        assert_eq!(window["dragDropEnabled"], false);
    }

    /// Windows adds a transparent window (for Mica and Acrylic) through its own
    /// config file; Tauri replaces the whole `windows` array when merging, so
    /// every other setting must match the base file or the two would drift.
    #[test]
    fn the_windows_window_differs_from_the_base_only_by_being_transparent() {
        let base: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid config");
        let windows: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json")).expect("valid config");
        let mut expected = base["app"]["windows"][0].clone();
        expected["transparent"] = serde_json::Value::Bool(true);
        assert_eq!(windows["app"]["windows"][0], expected);
        assert_eq!(windows["app"]["windows"].as_array().map(Vec::len), Some(1));
    }
}
