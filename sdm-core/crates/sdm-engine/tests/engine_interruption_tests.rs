//! Rust-only: pins what the engine does when a job is interrupted somewhere
//! Swift's tests do not reach — a mux cut short by a stop or a shutdown, and a
//! wholesale download paused — and the stability of the snapshot's
//! `generation` / `revision` counters.
mod support;

use std::sync::Arc;

use async_trait::async_trait;
use sdm_core::{
    Assembly, ComponentOrigin, DownloadItem, DownloadPackage, FileComponent, ItemState,
    MediaContainer, SourceUrl,
};
use sdm_engine::{
    DownloadEngine, FakeOrigin, FakeOriginBehavior, InMemoryStateStore, MuxError, Muxer,
    PersistedState, RetryPolicy, StateStore,
};
use sdm_resolve::ProcessRunError;
use support::{
    FakeWholesaleDownloader, RoutingTransport, engine_settings, make_scratch_directory,
    snapshot_item, test_payload, within,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// A muxer that signals it has started and then waits to be cancelled, the way
/// `ffmpeg` would be killed.
#[derive(Default)]
struct HangingMuxer {
    started: Notify,
    has_started: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl Muxer for HangingMuxer {
    async fn mux(
        &self,
        _video_part: &std::path::Path,
        _audio_part: &std::path::Path,
        _output: &std::path::Path,
        _container: &MediaContainer,
        cancel: &CancellationToken,
    ) -> Result<(), MuxError> {
        self.has_started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.started.notify_waiters();
        cancel.cancelled().await;
        Err(MuxError::Process(ProcessRunError::Cancelled))
    }
}

impl HangingMuxer {
    async fn wait_until_started(&self) {
        within(async {
            loop {
                let notified = self.started.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.has_started.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }
}

fn mux_item() -> DownloadItem {
    let video = FileComponent::new(
        SourceUrl::new("https://video.example.com/v.mp4").unwrap(),
        "clip.f137.mp4",
    );
    let audio = FileComponent::new(
        SourceUrl::new("https://audio.example.com/a.m4a").unwrap(),
        "clip.f140.m4a",
    );
    DownloadItem::new(vec![video, audio], "clip.mp4").with_assembly(Assembly::Mux)
}

fn mux_engine(
    dir: &std::path::Path,
    muxer: Arc<HangingMuxer>,
    store: Arc<InMemoryStateStore>,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(RoutingTransport::new(
            "video.example.com",
            test_payload(2000),
            FakeOriginBehavior::default(),
            "audio.example.com",
            test_payload(1000),
            FakeOriginBehavior::default(),
        )),
        store,
        engine_settings(1, 2, 8, dir),
        RetryPolicy::new(),
        None,
        Some(muxer),
        None,
    )
}

/// Stopping an item while `ffmpeg` runs must leave it stopped. Swift's `catch`
/// would have recorded the cancelled mux as `Failed("mux failed: ...")`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_an_item_during_its_mux_does_not_fail_it() {
    let dir = make_scratch_directory();
    let muxer = Arc::new(HangingMuxer::default());
    let engine = mux_engine(
        dir.path(),
        muxer.clone(),
        Arc::new(InMemoryStateStore::new()),
    );
    let item = mux_item();
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    muxer.wait_until_started().await;
    assert!(snapshot_item(id, &engine).await.unwrap().is_assembling);

    engine.stop_item(id).await;
    within(engine.run_until_idle()).await.unwrap();

    let item = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(item.state, ItemState::Stopped);
    assert!(!item.is_assembling);
    assert_eq!(item.failed_attempt_count, None);
}

/// A shutdown during the mux must not write a failed item to the store.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutting_down_during_a_mux_does_not_fail_the_item() {
    let dir = make_scratch_directory();
    let muxer = Arc::new(HangingMuxer::default());
    let store = Arc::new(InMemoryStateStore::new());
    let engine = mux_engine(dir.path(), muxer.clone(), store.clone());
    let item = mux_item();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    muxer.wait_until_started().await;
    within(engine.shutdown()).await;

    let saved = store.load().await;
    assert_eq!(saved.packages[0].items[0].state, ItemState::Stopped);
}

fn wholesale_item() -> DownloadItem {
    let component = FileComponent::new(
        SourceUrl::new("https://example.com/stream.m3u8").unwrap(),
        "v.mp4",
    )
    .with_origin(ComponentOrigin::Wholesale {
        format_selector: "best".to_string(),
    });
    DownloadItem::new(vec![component], "v.mp4")
}

fn wholesale_engine(
    dir: &std::path::Path,
    downloader: Arc<FakeWholesaleDownloader>,
    store: Arc<InMemoryStateStore>,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(FakeOrigin::new(Vec::new())),
        store,
        engine_settings(1, 1, 8, dir),
        RetryPolicy::new(),
        None,
        None,
        Some(downloader),
    )
}

/// `WholesaleError::Cancelled` is the pause signal (Swift's
/// `CancellationError`), not a failure charged to the item.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_a_wholesale_item_lands_it_stopped_without_charging_an_attempt() {
    let dir = make_scratch_directory();
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(
        dir.path(),
        downloader.clone(),
        Arc::new(InMemoryStateStore::new()),
    );
    let item = wholesale_item();
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    within(downloader.wait_until_started()).await;
    engine.stop_item(id).await;
    within(engine.run_until_idle()).await.unwrap();

    let item = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(item.state, ItemState::Stopped);
    assert_eq!(item.failed_attempt_count, None);
    assert_eq!(downloader.call_count(), 1);
}

/// A shutdown mid-download goes through the same pause, and the store must not
/// say the item failed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutting_down_during_a_wholesale_download_persists_the_item_stopped() {
    let dir = make_scratch_directory();
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let store = Arc::new(InMemoryStateStore::new());
    let engine = wholesale_engine(dir.path(), downloader.clone(), store.clone());
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![wholesale_item()]))
        .await;

    within(downloader.wait_until_started()).await;
    within(engine.shutdown()).await;

    let saved: PersistedState = store.load().await;
    assert_eq!(saved.packages[0].items[0].state, ItemState::Stopped);
}

/// With nothing changing, the snapshot is the same value: same `generation`,
/// same package `revision`. A new package moves the generation and gets its
/// own revision without disturbing the first's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unchanged_snapshots_keep_their_generation_and_revisions() {
    let dir = make_scratch_directory();
    let engine = DownloadEngine::new(
        Arc::new(FakeOrigin::new(Vec::new())),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        None,
    );
    let disabled = |name: &str| support::example_item(name).with_state(ItemState::Stopped);
    engine
        .add(DownloadPackage::new("A").with_items(vec![disabled("a.bin")]))
        .await;

    let first = engine.snapshot().await;
    let again = engine.snapshot().await;
    assert!(first.content_eq(&again));
    assert_eq!(first.generation, again.generation);

    // A tick that changes nothing the snapshot shows leaves it alone, too.
    engine.tick().await;
    let after_tick = engine.snapshot().await;
    assert_eq!(first.generation, after_tick.generation);

    engine
        .add(DownloadPackage::new("B").with_items(vec![disabled("b.bin")]))
        .await;
    let second = engine.snapshot().await;
    assert!(second.generation > first.generation);
    assert_eq!(second.packages[0].revision, first.packages[0].revision);
    assert!(second.packages[1].revision > first.packages[0].revision);

    // Changing an item changes only its package's revision.
    let id = second.packages[1].items[0].id;
    engine
        .set_priority(Some(sdm_core::Priority::High), id)
        .await;
    engine.start_item(id).await;
    let third = engine.snapshot().await;
    assert_eq!(third.packages[0].revision, second.packages[0].revision);
    assert!(third.packages[1].revision > second.packages[1].revision);
    assert!(third.generation > second.generation);
}

/// A stop (or disable) that lands just as a transfer finishes must not undo
/// the finish. The component tasks have already moved their files into place
/// by then, so landing the item `Stopped` left a finished file at the
/// destination with no sidecar behind it; the next start re-downloaded and
/// failed permanently with "A file already exists at the destination". Found
/// by `engine_byte_identity_under_churn`.
///
/// The stop is fired the moment the last byte is recorded, which is as close
/// to the window as a test outside the engine can get; whichever side of the
/// finish it lands on, the item must end `Completed` with the right bytes.
///
/// This is a statistical race test, not a deterministic pin: it fires the stop
/// over many rounds and can pass without ever landing inside the window. It
/// failed on the unfixed engine at round 0, but a pass proves less than it
/// looks like.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_that_races_the_end_of_a_transfer_never_poisons_the_next_start() {
    let payload = test_payload(300_000);
    for round in 0..150 {
        let dir = make_scratch_directory();
        let engine = support::new_engine(
            FakeOrigin::with_behavior(
                payload.clone(),
                FakeOriginBehavior {
                    chunk_size: 8192,
                    ..Default::default()
                },
            ),
            engine_settings(1, 2, 8, dir.path()).with_min_segment_size_bytes(64 * 1024),
        );
        let item = support::example_item("a.bin");
        let id = item.id;
        engine
            .add(DownloadPackage::new("Batch").with_items(vec![item]))
            .await;

        // Until every byte is recorded (or the item is already done), then stop.
        within(async {
            loop {
                let snapshot = snapshot_item(id, &engine).await.unwrap();
                if snapshot.state == ItemState::Completed {
                    break;
                }
                if snapshot.total_bytes == Some(payload.len() as i64)
                    && snapshot.completed.total_bytes() == payload.len() as i64
                {
                    engine.stop_item(id).await;
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;

        engine.resume_all().await;
        within(engine.run_until_idle()).await.unwrap();
        let landed = snapshot_item(id, &engine).await.unwrap().state;
        assert_eq!(landed, ItemState::Completed, "round {round}");
        assert_eq!(
            std::fs::read(dir.path().join("Batch").join("a.bin")).unwrap(),
            payload,
            "round {round}"
        );
        within(engine.shutdown()).await;
    }
}
