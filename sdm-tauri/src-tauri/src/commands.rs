//! Thin command wrappers over `sdm_app::App`. Each runs inside
//! `App::guarded` so a panic becomes an error the window can show.

use std::sync::Arc;

use sdm_app::host::{PatchSink, SinkClosed};
use sdm_app::protocol::{
    BinariesStatusDto, DeletionInfo, DeletionTarget, HandoffTarget, Patch, SettingsDto, ThemeInfo,
};
use sdm_app::{App, AppError};
use tauri::State;
use tauri::ipc::Channel;

pub struct ChannelSink(pub Channel<Patch>);

impl PatchSink for ChannelSink {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed> {
        self.0.send(patch).map_err(|_| SinkClosed)
    }
}

type Shared<'a> = State<'a, Arc<App>>;

#[tauri::command]
#[specta::specta]
pub async fn attach(app: Shared<'_>, on_patch: Channel<Patch>) -> Result<(), AppError> {
    let sink: Arc<dyn PatchSink> = Arc::new(ChannelSink(on_patch));
    app.guarded(move |app| async move { app.attach(sink).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_url(
    app: Shared<'_>,
    url: String,
    package_name: Option<String>,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.add_url(&url, package_name.as_deref()).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn start_items(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.start_items(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_items(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.stop_items(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_items(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.retry_items(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_mux_items(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.retry_mux_items(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn reset_items(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.reset_items(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_items_enabled(
    app: Shared<'_>,
    ids: Vec<String>,
    enabled: bool,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.set_items_enabled(&ids, enabled).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_items(
    app: Shared<'_>,
    ids: Vec<String>,
    delete_files: bool,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.remove_items(&ids, delete_files).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_package(
    app: Shared<'_>,
    id: String,
    delete_files: bool,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.remove_package(&id, delete_files).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn move_items(
    app: Shared<'_>,
    ids: Vec<String>,
    package_id: String,
    at_index: Option<u32>,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.move_items(&ids, &package_id, at_index).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn move_items_to_new_package(
    app: Shared<'_>,
    ids: Vec<String>,
    name: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.move_items_to_new_package(&ids, &name).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_items(
    app: Shared<'_>,
    ids: Vec<String>,
    package_id: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.reorder_items(&ids, &package_id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_packages(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.reorder_packages(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn describe_deletion(
    app: Shared<'_>,
    target: DeletionTarget,
) -> Result<DeletionInfo, AppError> {
    app.guarded(move |app| async move { app.describe_deletion(&target).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn open_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.open_item(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.reveal_item(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_package(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.reveal_package(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn pause_all(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.pause_all().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_all(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.resume_all().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_list_visible(app: Shared<'_>, visible: bool) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.set_list_visible(visible).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_detail_items(
    app: Shared<'_>,
    owner: String,
    ids: Vec<String>,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.set_detail_items(&owner, &ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn full_state(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.full_state().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(app: Shared<'_>) -> Result<SettingsDto, AppError> {
    Ok(app.get_settings())
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    handle: tauri::AppHandle,
    app: Shared<'_>,
    settings: SettingsDto,
) -> Result<(), AppError> {
    let previous = app.get_settings();
    let wanted = settings.launch_at_login;
    app.guarded(move |app| async move { app.update_settings(settings).await })
        .await?;
    if let Some(enabled) = crate::autostart::login_change(previous.launch_at_login, wanted)
        && let Err(error) = crate::autostart::apply(&handle, enabled)
    {
        // The OS refused: the toggle goes back to what actually took effect.
        let mut reverted = app.get_settings();
        reverted.launch_at_login = previous.launch_at_login;
        let _ = app.update_settings(reverted).await;
        return Err(error);
    }
    // The Dock / Menu Bar mode applies at once, with or without a window.
    #[cfg(target_os = "macos")]
    crate::activation::apply_current(&handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn binaries_status(app: Shared<'_>) -> Result<BinariesStatusDto, AppError> {
    Ok(app.binaries_status())
}

/// Looks for a newer yt-dlp now and waits for the check.
#[tauri::command]
#[specta::specta]
pub async fn check_binaries_now(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.check_binaries_now().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_themes(app: Shared<'_>) -> Result<Vec<ThemeInfo>, AppError> {
    Ok(app.list_themes())
}

/// Opens the Settings window, or focuses it if it is open. Async because a
/// window created from a synchronous command deadlocks on Windows.
#[tauri::command]
#[specta::specta]
pub async fn open_settings(handle: tauri::AppHandle) -> Result<(), AppError> {
    crate::settings_window::open(&handle)
}

/// Where the Chrome extension stands: reporting in, its version, the cookie
/// file's age and the folder to load unpacked.
#[tauri::command]
#[specta::specta]
pub async fn chrome_extension_status(
    handle: tauri::AppHandle,
) -> Result<sdm_app::protocol::ChromeExtensionDto, AppError> {
    crate::chrome_setup::status(&handle)
}

/// Copies the extension to its stable folder and registers the host with
/// Chrome; returns the folder.
#[tauri::command]
#[specta::specta]
pub async fn prepare_chrome_extension(handle: tauri::AppHandle) -> Result<String, AppError> {
    crate::chrome_setup::prepare(&handle)
}

/// Opens Chrome on `chrome://extensions/`.
#[tauri::command]
#[specta::specta]
pub async fn open_chrome_extensions() -> Result<(), AppError> {
    crate::chrome_setup::open_chrome_extensions()
}

/// Shows the extension folder in the file manager.
#[tauri::command]
#[specta::specta]
pub async fn reveal_chrome_extension_folder(handle: tauri::AppHandle) -> Result<(), AppError> {
    let folder = crate::chrome_setup::prepare(&handle)?;
    tauri_plugin_opener::reveal_item_in_dir(folder)
        .map_err(|e| AppError::failed(format!("could not show the folder: {e}")))
}

/// Opens the Chrome extension setup window. Async because a window created
/// from a synchronous command deadlocks on Windows.
#[tauri::command]
#[specta::specta]
pub async fn open_chrome_setup(handle: tauri::AppHandle) -> Result<(), AppError> {
    crate::chrome_setup_window::open(&handle)
}

/// What the tray popover shows now.
#[tauri::command]
#[specta::specta]
pub async fn tray_status(app: Shared<'_>) -> Result<sdm_app::tray::TrayStatus, AppError> {
    Ok(app.tray_status())
}

/// The popover's Quit: goes through the quit guard.
#[tauri::command]
#[specta::specta]
pub async fn quit_requested(handle: tauri::AppHandle) -> Result<(), AppError> {
    crate::lifecycle::request_quit(&handle);
    Ok(())
}

/// The page calls this once its navigation listener is on: it returns the tab
/// asked for while the page was loading, if any, and from then on requests are
/// sent as events.
#[tauri::command]
#[specta::specta]
pub async fn take_pending_route(
    state: State<'_, crate::main_window::NavigationState>,
) -> Result<Option<String>, AppError> {
    Ok(state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .page_ready())
}

/// Shows the main window, on the tab `route` names when it names one.
#[tauri::command]
#[specta::specta]
pub async fn show_main(handle: tauri::AppHandle, route: Option<String>) -> Result<(), AppError> {
    crate::main_window::show(&handle, route.as_deref())
}

/// Previews a theme in every window; `None` goes back to the stored theme.
#[tauri::command]
#[specta::specta]
pub async fn broadcast_theme(
    handle: tauri::AppHandle,
    theme_id: Option<String>,
) -> Result<(), AppError> {
    crate::settings_window::broadcast_theme(&handle, theme_id)
}

#[tauri::command]
#[specta::specta]
pub async fn theme_css(app: Shared<'_>, theme_id: Option<String>) -> Result<String, AppError> {
    app.theme_css(theme_id.as_deref())
}

/// Whether the window has a translucent backdrop (Mica or Acrylic), so the
/// frontend lets its sidebar and toolbar show it.
#[tauri::command]
#[specta::specta]
pub async fn vibrancy_active(
    state: State<'_, crate::vibrancy::VibrancyState>,
) -> Result<bool, AppError> {
    Ok(state.0.is_some())
}

// MARK: Link grabber

#[tauri::command]
#[specta::specta]
pub async fn grabber_ingest_text(app: Shared<'_>, text: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_ingest_text(&text).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_remove_links(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_remove_links(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_remove_package(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_remove_package(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_clear(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_clear().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_move_link(
    app: Shared<'_>,
    id: String,
    package_id: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_move_link(&id, &package_id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_rename_package(
    app: Shared<'_>,
    id: String,
    name: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_rename_package(&id, &name).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_merge_packages(
    app: Shared<'_>,
    source: String,
    destination: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_merge_packages(&source, &destination).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_split_package(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_split_package(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_recheck(app: Shared<'_>, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_recheck(&ids).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_recheck_failed(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_recheck_failed().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_cancel_checks(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_cancel_checks().await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_set_format(
    app: Shared<'_>,
    id: String,
    option_id: String,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_set_format(&id, &option_id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_copy_url(app: Shared<'_>, id: String) -> Result<String, AppError> {
    app.guarded(move |app| async move { app.grabber_copy_url(&id) })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn grabber_add_to_downloads(
    app: Shared<'_>,
    target: HandoffTarget,
    start: bool,
) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.grabber_add_to_downloads(target, start).await })
        .await
}
