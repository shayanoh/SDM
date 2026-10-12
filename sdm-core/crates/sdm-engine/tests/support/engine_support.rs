//! Port of the engine test helpers in Swift's `TestSupport.swift` and
//! `DownloadEngineTests.swift`: a gate a test can park a download at, the
//! transport that honours it, and engine constructors.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use sdm_core::{ByteRange, DownloadItem, DownloadPackage, SourceUrl};
use sdm_engine::{
    DownloadEngine, EngineSettings, HttpTransport, InMemoryStateStore, ItemSnapshot, RangeRequest,
    RangeResponse, RetryPolicy, StateStore, TransportError, Trash,
};
use tokio::sync::Notify;
use uuid::Uuid;

use super::within;

/// Where a [`GatedOrigin`] parks a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatePhase {
    /// Block inside `fetch` for the one-byte probe, i.e. inside `prepare()`.
    Probe,
    /// Block halfway through a body stream, i.e. inside the worker's read.
    Body,
}

/// A flag a task can wait on without missing a wake-up that lands between
/// its check and its wait.
#[derive(Default)]
struct Flag {
    is_set: AtomicBool,
    notify: Notify,
}

impl Flag {
    fn set(&self) {
        self.is_set.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            // Registered before the flag is read, so a `set()` in between
            // cannot be missed.
            notified.as_mut().enable();
            if self.is_set.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
struct GateInner {
    open: Flag,
    arrival: Flag,
}

/// Lets a test park a download at an exact point and release it later,
/// without sleeping on a real clock. Swift's `Gate`.
#[derive(Clone, Default)]
pub struct Gate {
    inner: Arc<GateInner>,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn signal_arrival(&self) {
        self.inner.arrival.set();
    }

    /// Returns once a download has reached the gate.
    pub async fn wait_for_arrival(&self) {
        within(self.inner.arrival.wait()).await;
    }

    pub fn open(&self) {
        self.inner.open.set();
    }

    pub async fn wait_until_open(&self) {
        self.inner.open.wait().await;
    }
}

/// Swift's `GatedOrigin`.
pub struct GatedOrigin {
    payload: Bytes,
    gate: Gate,
    phase: GatePhase,
    /// Serve the whole body regardless of the requested range, and advertise
    /// no `Range` support — the shape that makes a download non-resumable.
    ignores_ranges: bool,
}

impl GatedOrigin {
    pub fn new(
        payload: impl Into<Bytes>,
        gate: Gate,
        phase: GatePhase,
        ignores_ranges: bool,
    ) -> Self {
        Self {
            payload: payload.into(),
            gate,
            phase,
            ignores_ranges,
        }
    }
}

#[async_trait]
impl HttpTransport for GatedOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let total = self.payload.len() as i64;
        let range = request.range.unwrap_or(ByteRange::new(0, total));
        let is_probe = range.start == 0 && range.end == 1;
        let status_code = if self.ignores_ranges { 200 } else { 206 };

        if is_probe {
            if self.phase == GatePhase::Probe {
                self.gate.signal_arrival();
                self.gate.wait_until_open().await;
            }
            return Ok(RangeResponse {
                status_code,
                total_size: Some(total),
                accepts_ranges: !self.ignores_ranges,
                validator: Some("etag-gated".to_string()),
                resolved_url: request.url,
                body: Box::pin(futures::stream::empty()),
            });
        }

        let (lower, upper) = if self.ignores_ranges {
            (0, self.payload.len())
        } else {
            (
                range.start.min(total) as usize,
                range.end.min(total) as usize,
            )
        };
        let slice = self.payload.slice(lower..upper);
        let body: futures::stream::BoxStream<'static, Result<Bytes, TransportError>> =
            if self.phase == GatePhase::Body {
                let split = slice.len() / 2;
                let first = (split > 0).then(|| Ok(slice.slice(..split)));
                let gate = self.gate.clone();
                let rest = futures::stream::once(async move {
                    gate.signal_arrival();
                    gate.wait_until_open().await;
                    Ok(slice.slice(split..))
                });
                futures::stream::iter(first).chain(rest).boxed()
            } else {
                futures::stream::once(async move { Ok(slice) }).boxed()
            };

        Ok(RangeResponse {
            status_code,
            total_size: Some(total),
            accepts_ranges: !self.ignores_ranges,
            validator: Some("etag-gated".to_string()),
            resolved_url: request.url,
            body,
        })
    }
}

/// A [`Trash`] that moves things into a scratch directory of its own instead
/// of the user's real Trash, and remembers what it was given. Every engine the
/// helpers below build uses one, so no test can reach the real Trash.
pub struct ScratchTrash {
    directory: tempfile::TempDir,
    counter: AtomicUsize,
    trashed: Mutex<Vec<PathBuf>>,
}

impl ScratchTrash {
    pub fn new() -> Self {
        Self {
            directory: tempfile::Builder::new()
                .prefix("sdm-trash-")
                .tempdir()
                .expect("create a scratch trash directory"),
            counter: AtomicUsize::new(0),
            trashed: Mutex::new(Vec::new()),
        }
    }

    /// Every path the engine asked to trash, in order.
    pub fn trashed(&self) -> Vec<PathBuf> {
        self.trashed.lock().unwrap().clone()
    }
}

impl Default for ScratchTrash {
    fn default() -> Self {
        Self::new()
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

/// Settings for the single-slot engines most tests want.
pub fn engine_settings(
    max_concurrent: usize,
    segments_per_item: usize,
    global_max_connections: usize,
    folder: &Path,
) -> EngineSettings {
    EngineSettings::new(
        max_concurrent,
        segments_per_item,
        global_max_connections,
        folder.to_path_buf(),
    )
}

/// An engine over `transport` with an in-memory store and the default retry
/// policy.
pub fn new_engine(
    transport: impl HttpTransport + 'static,
    settings: EngineSettings,
) -> DownloadEngine {
    new_engine_with_trash(transport, settings, Arc::new(ScratchTrash::new()))
}

/// [`new_engine`] over a state store the test holds on to, so it can read what
/// the engine saved or build a second engine over the same store.
pub fn new_engine_with_store(
    transport: impl HttpTransport + 'static,
    store: Arc<dyn StateStore>,
    settings: EngineSettings,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(transport),
        store,
        settings,
        RetryPolicy::new(),
        None,
        None,
        None,
    )
    .with_trash(Arc::new(ScratchTrash::new()))
}

/// [`new_engine`] with a trash the test can inspect.
pub fn new_engine_with_trash(
    transport: impl HttpTransport + 'static,
    settings: EngineSettings,
    trash: Arc<ScratchTrash>,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(transport),
        Arc::new(InMemoryStateStore::new()),
        settings,
        RetryPolicy::new(),
        None,
        None,
        None,
    )
    .with_trash(trash)
}

/// Swift's `makeGatedEngine`: one slot, one segment.
pub fn make_gated_engine(
    payload: Vec<u8>,
    folder: &Path,
    gate: &Gate,
    phase: GatePhase,
    ignores_ranges: bool,
    retry_policy: RetryPolicy,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(GatedOrigin::new(
            payload,
            gate.clone(),
            phase,
            ignores_ranges,
        )),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, folder),
        retry_policy,
        None,
        None,
        None,
    )
    .with_trash(Arc::new(ScratchTrash::new()))
}

/// One item's current snapshot, if the engine has it.
pub async fn snapshot_item(id: Uuid, engine: &DownloadEngine) -> Option<ItemSnapshot> {
    engine
        .snapshot()
        .await
        .packages
        .into_iter()
        .flat_map(|package| package.items)
        .find(|item| item.id == id)
}

/// An HTTP item for `https://example.com/<name>`.
pub fn example_item(name: &str) -> DownloadItem {
    DownloadItem::http(
        SourceUrl::new(format!("https://example.com/{name}")).expect("a valid URL"),
        name,
    )
}

/// Swift's `packageOf`: a package called `Batch` with one item per name,
/// positioned in order.
pub fn package_of(names: &[&str]) -> DownloadPackage {
    DownloadPackage::new("Batch").with_items(
        names
            .iter()
            .enumerate()
            .map(|(index, name)| example_item(name).with_position(index as i64))
            .collect(),
    )
}

/// Polls `condition` (yielding, never sleeping) until it holds; fails the
/// test instead of hanging if it never does. The async twin of
/// `support::wait_until`.
pub async fn wait_until_async<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    within(async {
        while !condition().await {
            tokio::task::yield_now().await;
        }
    })
    .await
}

/// Waits until `item` has recorded at least one completed byte.
pub async fn wait_for_progress(item: Uuid, engine: &DownloadEngine) {
    wait_until_async(|| async {
        snapshot_item(item, engine)
            .await
            .is_some_and(|snapshot| snapshot.completed.total_bytes() > 0)
    })
    .await;
}

/// The number of items currently `Running`.
pub async fn running_count(engine: &DownloadEngine) -> usize {
    engine
        .snapshot()
        .await
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .filter(|item| item.state == sdm_core::ItemState::Running)
        .count()
}
