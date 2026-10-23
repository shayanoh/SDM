//! Thin command wrappers over `sdm_app::App`. Each runs inside
//! `App::guarded` so a panic becomes an error the window can show.

use std::sync::Arc;

use sdm_app::host::{PatchSink, SinkClosed};
use sdm_app::protocol::{BinariesStatusDto, Patch, SettingsDto};
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
pub async fn start_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.start_item(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.stop_item(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.retry_item(&id).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_item(app: Shared<'_>, id: String, delete_file: bool) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.remove_item(&id, delete_file).await })
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
pub async fn update_settings(app: Shared<'_>, settings: SettingsDto) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.update_settings(settings).await })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn binaries_status(app: Shared<'_>) -> Result<BinariesStatusDto, AppError> {
    Ok(app.binaries_status())
}

#[tauri::command]
#[specta::specta]
pub async fn theme_css(app: Shared<'_>, theme_id: Option<String>) -> Result<String, AppError> {
    app.theme_css(theme_id.as_deref())
}
