//! Rust-only tests (no Swift twin): removing or resetting an item must not
//! race the scheduler.
//!
//! Swift's actor made "stop the runner, trash the files, drop the item" one
//! atomic sequence. Here the file work happens outside the engine lock, so
//! without a guard a `reconcile()` (the 1 Hz `tick`, an `add`, ...) landing
//! while the files are being trashed saw an enabled `Queued` item with no
//! runner and started a fresh task on those very files.
//!
//! Each test parks the operation inside `Trash::trash` (a real, deterministic
//! point after the old runner is gone and before the item is dropped or
//! reset), runs `tick()`, and checks that nothing started and nothing reached
//! the origin. Nothing sleeps; every wait is bounded.
mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use sdm_engine::{
    DownloadEngine, HttpTransport, InMemoryStateStore, RangeRequest, RangeResponse, RetryPolicy,
    TransportError, Trash,
};
use support::{
    Gate, GatePhase, GatedOrigin, ScratchTrash, engine_settings, make_scratch_directory,
    snapshot_item, test_payload, test_source, wait_for_progress, within,
};
use tokio::sync::Notify;
use uuid::Uuid;

/// A [`Trash`] whose first call parks the calling thread until the test
/// releases it. (`Trash::trash` is synchronous, so the park is a thread
/// block; the tests run on a four-thread runtime so the test body is
/// unaffected.)
struct ParkingTrash {
    inner: ScratchTrash,
    parked_once: AtomicBool,
    arrived: Notify,
    release: Mutex<Receiver<()>>,
}

impl ParkingTrash {
    fn new() -> (Arc<Self>, Sender<()>) {
        let (sender, receiver) = channel();
        (
            Arc::new(Self {
                inner: ScratchTrash::new(),
                parked_once: AtomicBool::new(false),
                arrived: Notify::new(),
                release: Mutex::new(receiver),
            }),
            sender,
        )
    }

    async fn wait_for_arrival(&self) {
        within(self.arrived.notified()).await;
    }
}

impl Trash for ParkingTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        if !self.parked_once.swap(true, Ordering::SeqCst) {
            self.arrived.notify_one();
            // A dropped sender also releases, so a failed test cannot hang
            // the runtime's shutdown.
            let _ = self.release.lock().unwrap().recv();
        }
        self.inner.trash(path)
    }
}

/// Counts every request that reaches the wrapped origin.
struct CountingOrigin {
    inner: GatedOrigin,
    requests: Arc<AtomicUsize>,
}

#[async_trait]
impl HttpTransport for CountingOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        self.inner.fetch(request).await
    }
}

const PAYLOAD: usize = 20_000;

struct Rig {
    engine: Arc<DownloadEngine>,
    trash: Arc<ParkingTrash>,
    release: Sender<()>,
    gate: Gate,
    requests: Arc<AtomicUsize>,
    package_id: Uuid,
    item_id: Uuid,
    dir: tempfile::TempDir,
}

/// One package with one download, running and parked mid-body.
async fn running_rig() -> Rig {
    let dir = make_scratch_directory();
    let gate = Gate::new();
    let requests = Arc::new(AtomicUsize::new(0));
    let (trash, release) = ParkingTrash::new();
    let engine = Arc::new(
        DownloadEngine::new(
            Arc::new(CountingOrigin {
                inner: GatedOrigin::new(
                    test_payload(PAYLOAD),
                    gate.clone(),
                    GatePhase::Body,
                    false,
                ),
                requests: requests.clone(),
            }),
            Arc::new(InMemoryStateStore::new()),
            engine_settings(1, 1, 8, dir.path()),
            RetryPolicy::new(),
            None,
            None,
            None,
        )
        .with_trash(trash.clone()),
    );
    let item = DownloadItem::http(test_source(), "a.bin");
    let item_id = item.id;
    let package_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("Batch")
                .with_id(package_id)
                .with_items(vec![item]),
        )
        .await;
    gate.wait_for_arrival().await;
    wait_for_progress(item_id, &engine).await;
    Rig {
        engine,
        trash,
        release,
        gate,
        requests,
        package_id,
        item_id,
        dir,
    }
}

/// With the operation parked at its trash step: a `tick()` (which
/// reconciles) must neither start the item nor touch the origin.
async fn assert_scheduler_leaves_it_alone(rig: &Rig, requests_before: usize) {
    rig.engine.tick().await;
    let state = snapshot_item(rig.item_id, &rig.engine)
        .await
        .map(|item| item.state);
    assert!(
        !matches!(state, Some(ItemState::Running)),
        "a tick started an item whose files are being trashed: {state:?}"
    );
    assert_eq!(
        rig.requests.load(Ordering::SeqCst),
        requests_before,
        "a request reached the origin while the item was being removed or reset"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tick_during_remove_item_does_not_restart_the_item() {
    let rig = running_rig().await;
    let engine = Arc::clone(&rig.engine);
    let item_id = rig.item_id;
    let removal = tokio::spawn(async move { engine.remove_item(item_id, true).await });
    rig.trash.wait_for_arrival().await;
    let before = rig.requests.load(Ordering::SeqCst);

    assert_scheduler_leaves_it_alone(&rig, before).await;

    rig.release.send(()).unwrap();
    within(removal).await.unwrap();
    assert!(rig.engine.snapshot().await.packages.is_empty());
    assert_eq!(rig.requests.load(Ordering::SeqCst), before);
    drop(rig.dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tick_during_remove_package_does_not_restart_its_items() {
    let rig = running_rig().await;
    let engine = Arc::clone(&rig.engine);
    let package_id = rig.package_id;
    let removal = tokio::spawn(async move { engine.remove_package(package_id, true).await });
    rig.trash.wait_for_arrival().await;
    let before = rig.requests.load(Ordering::SeqCst);

    assert_scheduler_leaves_it_alone(&rig, before).await;

    rig.release.send(()).unwrap();
    within(removal).await.unwrap();
    assert!(rig.engine.snapshot().await.packages.is_empty());
    assert_eq!(rig.requests.load(Ordering::SeqCst), before);
    drop(rig.dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tick_during_reset_download_does_not_restart_the_item_and_it_restarts_after() {
    let rig = running_rig().await;
    let engine = Arc::clone(&rig.engine);
    let item_id = rig.item_id;
    let reset = tokio::spawn(async move { engine.reset_download(item_id).await });
    rig.trash.wait_for_arrival().await;
    let before = rig.requests.load(Ordering::SeqCst);

    assert_scheduler_leaves_it_alone(&rig, before).await;

    // Let the reset finish and the restarted download run to the end: the
    // guard must not outlive the reset.
    rig.gate.open();
    rig.release.send(()).unwrap();
    within(reset).await.unwrap();
    within(rig.engine.run_until_idle()).await.unwrap();
    let item = snapshot_item(rig.item_id, &rig.engine).await.unwrap();
    assert_eq!(item.state, ItemState::Completed);
    assert!(rig.requests.load(Ordering::SeqCst) > before);
    let written = std::fs::read(rig.dir.path().join("Batch/a.bin")).unwrap();
    assert_eq!(written, test_payload(PAYLOAD));
}
