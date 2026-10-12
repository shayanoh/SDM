//! Port of Swift's `WholesaleDownloadTests.swift` (8 tests).
mod support;

use std::path::Path;
use std::sync::Arc;

use sdm_core::{
    AppTiming, Assembly, ComponentOrigin, DownloadItem, DownloadPackage, FileComponent, ItemState,
    Priority, SourceUrl, WholesaleError, WholesalePhase, WholesaleProgress,
};
use sdm_engine::{DownloadEngine, FakeOrigin, InMemoryStateStore, RetryPolicy};
use support::{
    FakeWholesaleDownloader, WorkerGate, WorkerGatedOrigin, engine_settings, example_item,
    make_scratch_directory, snapshot_item, test_payload, wait_until_async, within,
};

fn page() -> SourceUrl {
    SourceUrl::new("https://www.tiktok.com/@a/video/1").unwrap()
}

fn wholesale_item() -> DownloadItem {
    let component = FileComponent::new(page(), "Clip.mp4")
        .with_origin(ComponentOrigin::Wholesale {
            format_selector: "bv*+ba/b".to_string(),
        })
        .with_is_resumable(Some(false));
    DownloadItem::new(vec![component], "Clip.mp4")
        .with_source_url(page())
        .with_assembly(Assembly::None)
}

fn wholesale_engine(
    dir: &Path,
    downloader: Arc<FakeWholesaleDownloader>,
    max_concurrent: usize,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(FakeOrigin::new(Vec::new())),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(max_concurrent, 1, 8, dir),
        RetryPolicy::new(),
        None,
        None,
        Some(downloader),
    )
}

fn fragmented(downloaded: i64, total: i64) -> WholesaleProgress {
    WholesaleProgress {
        downloaded_bytes: Some(downloaded),
        total_bytes: Some(total),
        fraction: Some(0.4),
        phase: WholesalePhase::Downloading,
        is_fragmented: true,
    }
}

fn package(items: Vec<DownloadItem>) -> DownloadPackage {
    DownloadPackage::new("P").with_items(items)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wholesale_download_reports_synthesized_progress_then_completes() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;

    fake.emit(WholesaleProgress {
        downloaded_bytes: Some(500),
        total_bytes: Some(1000),
        phase: WholesalePhase::Downloading,
        ..Default::default()
    });
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Running);
    assert_eq!(snapshot.completed.total_bytes(), 500);
    assert_eq!(snapshot.total_bytes, Some(1000));
    assert_eq!(snapshot.is_resumable, Some(false));

    fake.emit(WholesaleProgress {
        phase: WholesalePhase::PostProcessing,
        ..Default::default()
    });
    assert!(snapshot_item(id, &engine).await.unwrap().is_assembling);

    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
    assert!(dir.path().join("P/Clip.mp4").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_fragment_progress_yields_a_moving_total_and_monotonic_bar() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;

    // HLS: no real total_bytes; fraction comes from fragment index/count.
    // total is derived as downloaded / fraction and moves each report.
    fake.emit(WholesaleProgress {
        downloaded_bytes: Some(1_000_000),
        fraction: Some(0.10),
        ..Default::default()
    });
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    let total1 = snapshot.total_bytes;
    let frac1 = snapshot.completed.total_bytes() as f64 / total1.unwrap_or(1) as f64;
    assert_eq!(total1, Some(10_000_000));
    assert!((frac1 - 0.10).abs() < 0.001);

    fake.emit(WholesaleProgress {
        downloaded_bytes: Some(3_000_000),
        fraction: Some(0.25),
        ..Default::default()
    });
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    let total2 = snapshot.total_bytes;
    let frac2 = snapshot.completed.total_bytes() as f64 / total2.unwrap_or(1) as f64;
    assert_eq!(total2, Some(12_000_000)); // estimate changed
    assert_ne!(total2, total1);
    assert!(frac2 > frac1); // bar advanced

    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_a_non_resumable_wholesale_download_shows_zero_and_reinvokes() {
    // No fragmented progress seen => the resumable native downloader is not
    // confirmed => the item stays non-resumable and its bar shows zero on
    // pause (its next attempt cannot rely on --continue).
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;
    fake.emit(WholesaleProgress {
        downloaded_bytes: Some(400),
        total_bytes: Some(1000),
        phase: WholesalePhase::Downloading,
        ..Default::default()
    });

    engine.stop_item(id).await;
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Stopped);
    assert_eq!(snapshot.is_resumable, Some(false));
    assert_eq!(snapshot.completed.total_bytes(), 0);

    assert_eq!(fake.call_count(), 1);
    engine.start_item(id).await;
    within(fake.wait_until_started()).await;
    assert_eq!(fake.call_count(), 2);
    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fragmented_progress_makes_a_wholesale_item_resumable() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;

    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().is_resumable,
        Some(false)
    );

    fake.emit(fragmented(400, 1000));
    engine.tick().await; // mirrors the probe result onto the item
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().is_resumable,
        Some(true)
    );

    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_a_resumable_wholesale_download_keeps_progress_and_resumes() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;
    fake.emit(fragmented(400, 1000));
    engine.tick().await;

    engine.stop_item(id).await;
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Stopped);
    assert_eq!(snapshot.is_resumable, Some(true));
    // Synthesized progress is retained — the next run's --continue resumes.
    assert_eq!(snapshot.completed.total_bytes(), 400);
    assert_eq!(fake.call_count(), 1);

    engine.start_item(id).await;
    within(fake.wait_until_started()).await;
    assert_eq!(fake.call_count(), 2);
    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resumable_wholesale_item_is_preempted_by_a_higher_priority_item() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let gate = WorkerGate::new();
    let engine = DownloadEngine::new(
        Arc::new(WorkerGatedOrigin::new(test_payload(4000), gate.clone())),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        Some(fake.clone()),
    );

    let hls = wholesale_item();
    let hls_id = hls.id;
    engine.add(package(vec![hls])).await;
    within(fake.wait_until_started()).await;
    fake.emit(fragmented(400, 1000));
    engine.tick().await;
    assert_eq!(
        snapshot_item(hls_id, &engine).await.unwrap().is_resumable,
        Some(true)
    );

    // A higher-priority plain item arrives; the wholesale item is now
    // resumable, so once hysteresis elapses it yields its slot rather than
    // holding it as a non-resumable item would.
    let plain = example_item("b.bin").with_priority(Priority::Highest);
    let plain_id = plain.id;
    engine
        .add(DownloadPackage::new("Q").with_items(vec![plain]))
        .await;
    for _ in 0..(AppTiming::TICKS_PER_SECOND * 5) {
        engine.tick().await;
    }

    // The wholesale item's downloader call is cancelled (and unwinds at once,
    // unlike Swift's), so wait for it to land queued.
    assert_eq!(
        snapshot_item(plain_id, &engine).await.unwrap().state,
        ItemState::Running
    );
    wait_until_async(|| async {
        snapshot_item(hls_id, &engine).await.unwrap().state == ItemState::Queued
    })
    .await;
    assert_eq!(fake.call_count(), 1);

    // Let plain finish; the wholesale item is re-desired and re-invoked
    // (its --continue resume), which we then let finish.
    let resume_fake = fake.clone();
    let resume_watch = tokio::spawn(async move {
        while resume_fake.call_count() < 2 {
            tokio::task::yield_now().await;
        }
        within(resume_fake.wait_until_started()).await;
        resume_fake.finish_success();
    });
    gate.open();
    within(engine.run_until_idle()).await.unwrap();
    within(resume_watch).await.unwrap();
    assert_eq!(fake.call_count(), 2);
    assert_eq!(
        snapshot_item(hls_id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_wholesale_item_reserves_its_scheduler_slot() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);

    let hls = wholesale_item();
    let plain = DownloadItem::http(page(), "b.bin");
    let (hls_id, plain_id) = (hls.id, plain.id);
    engine.add(package(vec![hls, plain])).await;
    within(fake.wait_until_started()).await;

    // The wholesale item is non-resumable, so pass 1 keeps its slot even
    // though `plain` outranks nothing here — it must not be preempted.
    engine.tick().await;
    assert_eq!(
        snapshot_item(hls_id, &engine).await.unwrap().state,
        ItemState::Running
    );
    assert_eq!(
        snapshot_item(plain_id, &engine).await.unwrap().state,
        ItemState::Queued
    );

    fake.finish_success();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_permanent_wholesale_error_fails_the_item() {
    let dir = make_scratch_directory();
    let fake = Arc::new(FakeWholesaleDownloader::new());
    let engine = wholesale_engine(dir.path(), fake.clone(), 1);
    let item = wholesale_item();
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(fake.wait_until_started()).await;

    fake.finish_failure(WholesaleError::AuthRequired);
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert!(
        matches!(snapshot.state, ItemState::Failed { .. }),
        "expected Failed, got {:?}",
        snapshot.state
    );
}
