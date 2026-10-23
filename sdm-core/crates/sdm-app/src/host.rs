//! What `sdm-app` needs from its surroundings. The Tauri shell implements
//! these; tests use the fakes at the bottom.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::notify::Notification;
use crate::protocol::Patch;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HostError(pub String);

pub trait Host: Send + Sync {
    /// Holds `state.json`, resume sidecars, `settings.json` and `bin/`.
    fn data_dir(&self) -> PathBuf;
    fn default_download_dir(&self) -> PathBuf;
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError>;
    fn notify(&self, notification: Notification) -> Result<(), HostError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

/// Where patches go. One ordered transport: `send` is called with a lock held
/// so patches reach the sink in `seq` order, and must not block.
pub trait PatchSink: Send + Sync {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed>;
}

/// A [`Host`] for tests: fixed directories, records notifications.
pub struct FakeHost {
    data_dir: PathBuf,
    download_dir: PathBuf,
    notifications: Mutex<Vec<Notification>>,
}

impl FakeHost {
    pub fn new(data_dir: PathBuf, download_dir: PathBuf) -> Self {
        Self {
            data_dir,
            download_dir,
            notifications: Mutex::new(Vec::new()),
        }
    }

    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Host for FakeHost {
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
    fn default_download_dir(&self) -> PathBuf {
        self.download_dir.clone()
    }
    fn reveal_in_file_manager(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
    fn notify(&self, notification: Notification) -> Result<(), HostError> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(notification);
        Ok(())
    }
}

/// A [`PatchSink`] for tests: keeps every patch; can be told to fail once.
#[derive(Default)]
pub struct RecordingSink {
    patches: Mutex<Vec<Patch>>,
    fail_next: Mutex<bool>,
}

impl RecordingSink {
    pub fn patches(&self) -> Vec<Patch> {
        self.patches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn take(&self) -> Vec<Patch> {
        std::mem::take(&mut *self.patches.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn fail_next(&self) {
        *self.fail_next.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

impl PatchSink for RecordingSink {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed> {
        let mut fail = self.fail_next.lock().unwrap_or_else(|e| e.into_inner());
        if std::mem::take(&mut *fail) {
            return Err(SinkClosed);
        }
        self.patches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(patch);
        Ok(())
    }
}
