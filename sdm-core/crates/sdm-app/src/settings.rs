//! The app's own settings, in `settings.json` inside the data directory. The
//! SwiftUI app keeps its settings in `UserDefaults`; the two apps share only
//! the download state.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::protocol::SettingsDto;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    download_folder: PathBuf,
}

pub struct SettingsStore {
    path: PathBuf,
    current: Stored,
}

impl SettingsStore {
    /// Reads `path`; a missing or unreadable file gives the defaults.
    pub fn load(path: PathBuf, default_folder: PathBuf) -> Self {
        let current = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice::<Stored>(&data).ok())
            .filter(|stored| stored.download_folder.is_absolute())
            .unwrap_or(Stored {
                download_folder: default_folder,
            });
        Self { path, current }
    }

    pub fn download_folder(&self) -> PathBuf {
        self.current.download_folder.clone()
    }

    pub fn get(&self) -> SettingsDto {
        SettingsDto {
            download_folder: self.current.download_folder.to_string_lossy().into_owned(),
        }
    }

    /// Validates, applies and persists. On any error nothing changes.
    pub fn update(&mut self, dto: SettingsDto) -> Result<(), AppError> {
        let folder = validate_folder(&dto.download_folder)?;
        let next = Stored {
            download_folder: folder,
        };
        self.write(&next)?;
        self.current = next;
        Ok(())
    }

    fn write(&self, stored: &Stored) -> Result<(), AppError> {
        let data = serde_json::to_vec_pretty(stored)
            .map_err(|e| AppError::failed(format!("could not encode the settings: {e}")))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::failed(format!("could not create {}: {e}", parent.display()))
            })?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, data)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .map_err(|e| AppError::failed(format!("could not save the settings: {e}")))
    }
}

/// The folder must be a non-empty absolute path that exists (or can be
/// created) and accepts a new file.
fn validate_folder(text: &str) -> Result<PathBuf, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("downloadFolder", "the folder is empty"));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::invalid(
            "downloadFolder",
            "the folder must be an absolute path",
        ));
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| AppError::invalid("downloadFolder", format!("cannot use that folder: {e}")))?;
    if !path.is_dir() {
        return Err(AppError::invalid(
            "downloadFolder",
            "that path is not a folder",
        ));
    }
    probe_writable(&path)?;
    Ok(path)
}

fn probe_writable(folder: &Path) -> Result<(), AppError> {
    let probe = folder.join(format!(".sdm-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| {
        AppError::invalid(
            "downloadFolder",
            format!("that folder is not writable: {e}"),
        )
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> SettingsStore {
        SettingsStore::load(
            dir.path().join("settings.json"),
            dir.path().join("Downloads"),
        )
    }

    #[test]
    fn a_missing_file_gives_the_default_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(store.download_folder(), dir.path().join("Downloads"));
    }

    #[test]
    fn a_valid_update_is_persisted_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        let mut s = store(&dir);
        s.update(SettingsDto {
            download_folder: target.to_string_lossy().into_owned(),
        })
        .unwrap();
        assert!(target.is_dir(), "the folder is created");
        let reloaded = store(&dir);
        assert_eq!(reloaded.download_folder(), target);
    }

    #[test]
    fn invalid_updates_are_rejected_and_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        for bad in ["", "   ", "relative/path", file.to_str().unwrap()] {
            let error = s
                .update(SettingsDto {
                    download_folder: bad.to_owned(),
                })
                .expect_err(bad);
            assert!(
                matches!(error, AppError::InvalidArgument { .. }),
                "{bad}: {error:?}"
            );
        }
        assert_eq!(
            s.download_folder(),
            dir.path().join("Downloads"),
            "old setting still in force"
        );
    }

    #[test]
    fn a_corrupt_file_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        assert_eq!(store(&dir).download_folder(), dir.path().join("Downloads"));
    }
}
