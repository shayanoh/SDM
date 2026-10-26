//! Port of Swift's `SnapshotReuseTests.swift` (12 tests).
//!
//! `snapshot()` hands back the previous snapshot — or the previous value of an
//! unchanged package — instead of rebuilding it. These pin the property that
//! makes that safe: a change made through any public mutation is visible in
//! the very next snapshot, and nothing else is.
//!
//! Swift's "storage reuse" tests assert that two arrays share a buffer. Rust
//! has no such identity, so those assert what the integers say instead:
//! `generation` is equal for the engine snapshot and `revision` is equal for a
//! package snapshot exactly when the engine handed back the previous value.
mod support;

use std::collections::HashSet;
use std::path::Path;

use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use sdm_engine::{DownloadEngine, FakeOrigin, InMemoryStateStore, LiveWindow};
use std::sync::Arc;
use support::{
    engine_settings, make_scratch_directory, new_engine_with_store, snapshot_item, test_payload,
    test_source, within,
};
use uuid::Uuid;

fn make_engine(
    dir: &Path,
    segments_per_item: usize,
    file_check_interval_ticks: usize,
) -> DownloadEngine {
    new_engine_with_store(
        FakeOrigin::new(test_payload(10)),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, segments_per_item, 8, dir)
            .with_file_check_interval_ticks(file_check_interval_ticks),
    )
}

/// Two stopped items in two packages: nothing runs, so the engine is idle.
async fn add_idle_packages(engine: &DownloadEngine) -> (Uuid, Uuid) {
    let first = DownloadItem::http(test_source(), "a.bin").with_state(ItemState::Stopped);
    let second = DownloadItem::http(test_source(), "b.bin").with_state(ItemState::Stopped);
    let ids = (first.id, second.id);
    engine
        .add(DownloadPackage::new("One").with_items(vec![first]))
        .await;
    engine
        .add(DownloadPackage::new("Two").with_items(vec![second]))
        .await;
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_engine_snapshots_stay_equal_across_ticks() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    add_idle_packages(&engine).await;

    let before = engine.snapshot().await;
    for _ in 0..10 {
        engine.tick().await;
    }
    assert_eq!(engine.snapshot().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reflects_enablement_changed_after_an_idle_snapshot() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, second) = add_idle_packages(&engine).await;

    assert!(snapshot_item(first, &engine).await.unwrap().is_enabled);
    engine.set_enabled(false, first).await;
    assert!(!snapshot_item(first, &engine).await.unwrap().is_enabled);
    // And the untouched item is exactly as it was.
    assert!(snapshot_item(second, &engine).await.unwrap().is_enabled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reflects_a_segment_override_set_after_an_idle_snapshot() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, second) = add_idle_packages(&engine).await;

    assert_eq!(
        snapshot_item(first, &engine)
            .await
            .unwrap()
            .configured_segments,
        2
    );
    engine.set_segment_count(5, first).await;
    assert_eq!(
        snapshot_item(first, &engine)
            .await
            .unwrap()
            .configured_segments,
        5
    );
    assert_eq!(
        snapshot_item(second, &engine)
            .await
            .unwrap()
            .configured_segments,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reflects_new_settings_applied_after_an_idle_snapshot() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, _) = add_idle_packages(&engine).await;

    assert_eq!(
        snapshot_item(first, &engine)
            .await
            .unwrap()
            .configured_segments,
        2
    );
    engine
        .update_settings(engine_settings(1, 7, 8, dir.path()))
        .await;
    assert_eq!(
        snapshot_item(first, &engine)
            .await
            .unwrap()
            .configured_segments,
        7
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reflects_removal_after_an_idle_snapshot() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, second) = add_idle_packages(&engine).await;

    assert_eq!(engine.snapshot().await.packages.len(), 2);
    engine.remove_item(first, false).await;
    let after = engine.snapshot().await;
    let ids: Vec<Uuid> = after
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .map(|item| item.id)
        .collect();
    assert_eq!(ids, [second]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reflects_a_download_started_and_finished_after_an_idle_snapshot() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, second) = add_idle_packages(&engine).await;

    assert_eq!(
        snapshot_item(first, &engine).await.unwrap().state,
        ItemState::Stopped
    );
    engine.start_item(first).await;
    within(engine.run_until_idle()).await.unwrap();

    let finished = snapshot_item(first, &engine).await.unwrap();
    assert_eq!(finished.state, ItemState::Completed);
    assert_eq!(finished.completed.total_bytes(), 10);
    assert_eq!(
        snapshot_item(second, &engine).await.unwrap().state,
        ItemState::Stopped
    );
}

// MARK: - File-missing cadence

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_file_is_noticed_only_when_the_check_interval_comes_round() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 3);
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    // The first tick always checks; the file is there.
    engine.tick().await;
    assert!(!snapshot_item(id, &engine).await.unwrap().file_missing);

    std::fs::remove_file(dir.path().join("Batch").join("a.bin")).unwrap();

    // Two more ticks are still inside the interval: not re-checked yet.
    engine.tick().await;
    engine.tick().await;
    assert!(!snapshot_item(id, &engine).await.unwrap().file_missing);

    // The third closes it.
    engine.tick().await;
    assert!(snapshot_item(id, &engine).await.unwrap().file_missing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_clears_a_stale_missing_file_flag_before_the_next_check() {
    let dir = make_scratch_directory();
    // A long interval, so only the first tick's check ever runs here.
    let engine = make_engine(dir.path(), 2, 1000);
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    std::fs::remove_file(dir.path().join("Batch").join("a.bin")).unwrap();
    engine.tick().await;
    assert!(snapshot_item(id, &engine).await.unwrap().file_missing);

    // Download it again: the freshly written file must not inherit the flag.
    engine.reset_download(id).await;
    engine.start_item(id).await;
    within(engine.run_until_idle()).await.unwrap();

    let redone = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(redone.state, ItemState::Completed);
    assert!(!redone.file_missing);
}

// MARK: - Reuse
//
// Swift compares array storage identity here. The Rust snapshot carries
// integers that say the same thing: an equal `generation` for the engine and
// an equal `revision` for a package mean the previous value was handed back.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_engine_returns_the_same_snapshot_across_ticks() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    add_idle_packages(&engine).await;

    // The first tick runs the one-off file check; settle past it.
    engine.tick().await;
    let before = engine.snapshot().await;
    for _ in 0..10 {
        engine.tick().await;
    }
    let after = engine.snapshot().await;
    // A tick with nothing running must write nothing a snapshot reads.
    assert_eq!(before.generation, after.generation);
    assert!(before.content_eq(&after));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unchanged_package_keeps_its_revision_when_another_package_changes() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let (first, _) = add_idle_packages(&engine).await;

    let before = engine.snapshot().await;
    engine.set_enabled(false, first).await;
    let after = engine.snapshot().await;

    assert_ne!(before.packages[0].revision, after.packages[0].revision);
    assert!(!after.packages[0].items[0].is_enabled);
    assert_eq!(before.packages[1].revision, after.packages[1].revision);
    assert_ne!(before.generation, after.generation);
}

// MARK: - Display-sized history

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_carries_only_the_recent_history_while_the_engine_keeps_the_full_one() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    // One real tick folds the transferred bytes in; the rest are idle and
    // add a single zero, so the full history is exactly two samples.
    for _ in 0..5 {
        engine.tick().await;
    }

    let short = snapshot_item(id, &engine).await.unwrap().speed_history;
    let full = engine.speed_histories(&HashSet::from([id])).await[&id].clone();
    assert!(short.len() <= LiveWindow::ITEM_HISTORY_SAMPLES);
    let tail = &full[full.len().saturating_sub(LiveWindow::ITEM_HISTORY_SAMPLES)..];
    assert_eq!(short, tail);
    assert!(full.iter().sum::<f64>() > 0.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn speed_histories_omit_unknown_items() {
    let dir = make_scratch_directory();
    let engine = make_engine(dir.path(), 2, 1);
    assert!(
        engine
            .speed_histories(&HashSet::from([Uuid::new_v4()]))
            .await
            .is_empty()
    );
}

// MARK: - Beyond the Swift suite

/// Rust-only: with nothing running, 100 ticks — long enough for the default
/// five-second file check to come round more than once — leave the engine
/// `generation` and every package `revision` exactly as they were. One
/// package holds a stopped item, the other a finished one whose file is
/// checked on that cadence.
///
/// A few ticks come first: a download that just finished still owes its
/// sampler one real window (the bytes it moved), which legitimately changes
/// the snapshot once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_engine_serves_the_same_generation() {
    let dir = make_scratch_directory();
    let engine = new_engine_with_store(
        FakeOrigin::new(test_payload(10)),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 2, 8, dir.path()),
    );
    let stopped = DownloadItem::http(test_source(), "a.bin").with_state(ItemState::Stopped);
    let finished = DownloadItem::http(test_source(), "b.bin");
    let finished_id = finished.id;
    engine
        .add(DownloadPackage::new("One").with_items(vec![stopped]))
        .await;
    engine
        .add(DownloadPackage::new("Two").with_items(vec![finished]))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(finished_id, &engine).await.unwrap().state,
        ItemState::Completed
    );
    // Long enough for the finished item's short history window to slide down
    // to zeros, which is the last thing a snapshot reads that changes.
    for _ in 0..(LiveWindow::ITEM_HISTORY_SAMPLES + 5) {
        engine.tick().await;
    }

    let before = engine.snapshot().await;
    for _ in 0..100 {
        engine.tick().await;
    }
    let after = engine.snapshot().await;

    assert_eq!(before.generation, after.generation);
    let revisions = |snapshot: &sdm_engine::EngineSnapshot| -> Vec<u64> {
        snapshot.packages.iter().map(|p| p.revision).collect()
    };
    assert_eq!(revisions(&before), revisions(&after));
    assert!(before.content_eq(&after));
}
