//! Port of Swift's `NonResumableRestartTests.swift` (2 tests).
mod support;

use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use sdm_engine::{FakeOrigin, FakeOriginBehavior};
use support::{
    engine_settings, make_scratch_directory, new_engine, snapshot_item, test_payload, test_source,
    within,
};

/// Verifies both halves of item 4's fix: `DownloadTask::prepare()` already
/// discarded on-disk bytes correctly for a non-resumable restart (see
/// `resume_tests`'s validator-mismatch coverage for the equivalent byte-level
/// proof); what was missing was `DownloadEngine` itself not carrying the old
/// task's stale partial `completed` forward once the item lands somewhere
/// other than `Completed`.
///
/// Drives this through a genuine transient failure (a dropped connection
/// after partial bytes) rather than an explicit `stop_item()` call: both land
/// in `run_item`'s `None` retire-intent branch and reach `finish_item` the
/// same way, but a synchronous, scripted `FakeOrigin` failure is
/// deterministic, whereas racing an explicit `stop_item()` against a worker's
/// own in-flight write is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transient_failure_on_a_non_resumable_download_discards_its_displayed_progress() {
    let dir = make_scratch_directory();
    let behavior = FakeOriginBehavior {
        ignores_ranges: true,
        chunk_size: 64,
        drop_after_bytes: Some(1000),
        ..Default::default()
    };
    let engine = new_engine(
        FakeOrigin::with_behavior(test_payload(8000), behavior),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    // The dropped connection is transient, so the item is held in retry
    // backoff rather than promoted back to `Running` — `run_until_idle()`
    // (which doesn't tick, so backoff never ages) settles on exactly that
    // state without racing a real retry attempt.
    within(engine.run_until_idle()).await.unwrap();

    let last = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(last.state, ItemState::Queued);
    assert_eq!(last.is_resumable, Some(false));
    assert_eq!(last.completed.total_bytes(), 0);
}

/// Regression guard for the fix above: a non-resumable download that
/// genuinely finishes must keep its full `completed` range, not have it
/// zeroed just because `is_resumable == Some(false)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_resumable_download_that_completes_keeps_its_full_completed_range() {
    let dir = make_scratch_directory();
    let behavior = FakeOriginBehavior {
        ignores_ranges: true,
        ..Default::default()
    };
    let payload = test_payload(4000);
    let engine = new_engine(
        FakeOrigin::with_behavior(payload.clone(), behavior),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
    assert_eq!(snapshot.is_resumable, Some(false));
    assert_eq!(snapshot.completed.total_bytes(), payload.len() as i64);
}
