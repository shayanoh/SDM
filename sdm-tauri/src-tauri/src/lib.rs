mod commands;
mod host;

use std::sync::Arc;
use std::time::Duration;

use commands::*;
use sdm_app::host::Host;
use sdm_app::simulation::{SimulationHost, populate, simulated_dependencies};
use sdm_app::{App, AppError};
use sdm_core::AppTiming;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};

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
        theme_css,
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
        .invoke_handler(builder.invoke_handler())
        .setup(move |tauri_app| {
            builder.mount_events(tauri_app);
            let (app, simulation) = build_app(tauri_app.handle())?;
            tauri_app.manage(Arc::clone(&app));

            // Restore, then provision binaries in the background, then beat.
            let heartbeat_app = Arc::clone(&app);
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
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the SDM application");
}

#[cfg(all(test, debug_assertions))]
mod tests {
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
