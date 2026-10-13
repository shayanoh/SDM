//! Helpers shared by the FFI integration tests. Each test file pulls this in
//! with `mod support;`, so any one file uses only part of it.
#![allow(dead_code, unused_imports)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sdm_core::{DownloadItem, DownloadPackage, SourceUrl};
use sdm_engine::{
    EngineSettings, FakeOrigin, HttpTransport, InMemoryStateStore, JsonStateStore, RetryPolicy,
    StateStore, Trash,
};
use sdm_ffi::{DownloadEngine, EngineDependencies};
use uuid::Uuid;

/// How long a test may run before it fails instead of hanging.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(TEST_TIMEOUT, future)
        .await
        .expect("timed out: the test hung")
}

/// Polls `condition` (yielding between polls, never sleeping on a real
/// clock) until it holds.
pub async fn wait_until<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    within(async {
        while !condition().await {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

pub fn scratch_directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("sdm-ffi-tests-")
        .tempdir()
        .expect("create a scratch directory")
}

pub fn su(text: &str) -> SourceUrl {
    SourceUrl::new(text).expect("a valid URL")
}

/// A [`Trash`] that moves things into a scratch directory of its own, so no
/// test reaches the user's real Trash.
pub struct ScratchTrash {
    directory: tempfile::TempDir,
    counter: AtomicUsize,
    trashed: Mutex<Vec<PathBuf>>,
}

impl ScratchTrash {
    pub fn new() -> Self {
        Self {
            directory: tempfile::Builder::new()
                .prefix("sdm-ffi-trash-")
                .tempdir()
                .expect("create a scratch trash"),
            counter: AtomicUsize::new(0),
            trashed: Mutex::new(Vec::new()),
        }
    }

    pub fn trashed(&self) -> Vec<PathBuf> {
        self.trashed.lock().unwrap().clone()
    }
}

impl Trash for ScratchTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        let slot = self.directory.path().join(format!(
            "{}-{}",
            self.counter.fetch_add(1, Ordering::SeqCst),
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::rename(path, slot)?;
        self.trashed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

pub fn engine_settings(folder: &Path) -> EngineSettings {
    EngineSettings::new(2, 2, 8, folder.to_path_buf())
}

pub fn payload(count: usize) -> Vec<u8> {
    (0..count)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect()
}

pub fn dependencies(
    transport: impl HttpTransport + 'static,
    state_store: Arc<dyn StateStore>,
    trash: Arc<dyn Trash>,
) -> EngineDependencies {
    EngineDependencies {
        transport: Arc::new(transport),
        state_store,
        trash,
        resolver: None,
        muxer: None,
        wholesale_downloader: None,
        retry_policy: RetryPolicy::new(),
    }
}

/// An engine over a fake origin serving `payload`, an in-memory store and a
/// scratch trash, downloading into `folder`.
pub fn engine_over(payload: Vec<u8>, folder: &Path) -> (Arc<DownloadEngine>, Arc<ScratchTrash>) {
    let trash = Arc::new(ScratchTrash::new());
    let engine = DownloadEngine::with_dependencies(
        engine_settings(folder),
        dependencies(
            FakeOrigin::new(payload),
            Arc::new(InMemoryStateStore::new()),
            trash.clone(),
        ),
        DownloadEngine::default_tools(),
    )
    .expect("build the engine");
    (engine, trash)
}

/// A one-item package. `enabled == false` keeps it from ever starting.
pub fn package(name: &str, file: &str, enabled: bool) -> DownloadPackage {
    let item =
        DownloadItem::http(su(&format!("https://example.com/{file}")), file).with_enabled(enabled);
    DownloadPackage::new(name).with_items(vec![item])
}
