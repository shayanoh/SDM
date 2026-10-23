mod commands;
mod host;

use std::sync::Arc;
use std::time::Duration;

use commands::*;
use sdm_app::App;
use sdm_core::AppTiming;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        attach,
        add_url,
        start_item,
        stop_item,
        retry_item,
        remove_item,
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
            let host = Arc::new(host::TauriHost::new(tauri_app.handle())?);
            let app = App::production(host)?;
            tauri_app.manage(Arc::clone(&app));

            // Restore, then provision binaries in the background, then beat.
            let heartbeat_app = Arc::clone(&app);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = heartbeat_app.restore().await {
                    tracing::error!("could not restore saved downloads: {error}");
                }
                let provisioner = Arc::clone(&heartbeat_app);
                tauri::async_runtime::spawn(async move { provisioner.provision_binaries().await });

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
