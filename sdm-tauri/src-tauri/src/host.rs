//! The Tauri implementation of `sdm_app::Host`.

use std::path::{Path, PathBuf};

use sdm_app::host::{Host, HostError};
use sdm_app::notify::Notification;
use tauri::{AppHandle, Manager};

pub struct TauriHost {
    data_dir: PathBuf,
    download_dir: PathBuf,
    bundled_tools_dir: Option<PathBuf>,
    app: AppHandle,
}

impl TauriHost {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let paths = app.path();
        // `data_dir()` is ~/Library/Application Support on macOS (shared with
        // the SwiftUI app, decision D5), %APPDATA% on Windows and
        // $XDG_DATA_HOME or ~/.local/share on Linux.
        let data_dir = paths.data_dir().map_err(|e| e.to_string())?.join("SDM");
        // The user's Downloads folder itself, as in the SwiftUI app.
        let download_dir = paths.download_dir().map_err(|e| e.to_string())?;
        // Only the macOS bundle ships tools (see `tauri.macos.conf.json`).
        let bundled_tools_dir = if cfg!(target_os = "macos") {
            paths.resource_dir().ok().map(|dir| dir.join("vendor"))
        } else {
            None
        };
        Ok(Self {
            data_dir,
            download_dir,
            bundled_tools_dir,
            app: app.clone(),
        })
    }
}

impl Host for TauriHost {
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn default_download_dir(&self) -> PathBuf {
        self.download_dir.clone()
    }

    fn bundled_tools_dir(&self) -> Option<PathBuf> {
        self.bundled_tools_dir.clone()
    }

    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError> {
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| HostError(e.to_string()))
    }

    fn open_path(&self, path: &Path) -> Result<(), HostError> {
        use tauri_plugin_opener::OpenerExt;
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| HostError(e.to_string()))
    }

    fn notify(&self, notification: Notification) -> Result<(), HostError> {
        // Spec 5 turns this into an OS notification.
        let _ = &self.app;
        match notification {
            Notification::Finished { filename } => tracing::info!("finished: {filename}"),
            Notification::Failed { filename, reason } => {
                tracing::info!("failed: {filename}: {reason}")
            }
        }
        Ok(())
    }
}
