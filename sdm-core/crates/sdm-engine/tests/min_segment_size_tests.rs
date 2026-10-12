//! Port of Swift's `MinSegmentSizeTests.swift` (3 tests).
//!
//! `EngineSettings::min_segment_size_bytes` is the user-facing floor; it is
//! threaded straight into `DownloadTaskConfiguration::min_chunk`, whose own
//! splitting behavior is already covered at the `DownloadTask` level (see
//! `resegmentation_tests`). These tests instead prove the *engine* actually
//! passes the configured value through, rather than a hardcoded constant, by
//! observing how many segments a single item is allowed to split into.
mod support;

use sdm_core::DownloadPackage;
use sdm_engine::{DownloadEngine, EngineSettings};
use support::{
    WorkerGate, WorkerGatedOrigin, example_item, make_scratch_directory, new_engine, snapshot_item,
    test_payload, wait_until_async, within,
};
use uuid::Uuid;

async fn active_segments(item_id: Uuid, engine: &DownloadEngine) -> usize {
    snapshot_item(item_id, engine)
        .await
        .map_or(0, |item| item.active_segments)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn min_segment_size_bytes_prevents_splitting_below_the_floor() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let payload = test_payload(8000);
    let engine = new_engine(
        WorkerGatedOrigin::new(payload.clone(), gate.clone()),
        EngineSettings::new(1, 4, 32, dir.path().to_path_buf())
            .with_min_segment_size_bytes(payload.len() as i64),
    );
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    // Swift yields 50 times and then reads the count; on a multi-threaded
    // runtime that alone does not prove the lone worker had started, so first
    // wait until it holds its claim.
    wait_until_async(|| async { active_segments(id, &engine).await == 1 }).await;

    // A floor as large as the whole file means splitting the sole worker's
    // claim for any of the other three configured segments would drop below
    // it, so they must never actually claim anything, no matter how long we
    // wait.
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(active_segments(id, &engine).await, 1);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn min_segment_size_bytes_allows_splitting_above_the_floor() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(8000), gate.clone()),
        EngineSettings::new(1, 4, 32, dir.path().to_path_buf()).with_min_segment_size_bytes(64),
    );
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    wait_until_async(|| async { active_segments(id, &engine).await >= 4 }).await;

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[test]
fn default_min_segment_size_is_ten_megabytes() {
    let settings = EngineSettings::new(1, 1, 1, "/tmp".into());
    assert_eq!(settings.min_segment_size_bytes, 10 * 1024 * 1024);
}
