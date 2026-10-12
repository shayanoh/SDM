//! Port of Swift's `EngineRetryTests.swift` (6 tests): spec §6.4's failure
//! handling, at the engine level.
//!
//! The shape being guarded against: `run()` returned every transient failure
//! to `Queued`, `tick()` reconciled every second, the scheduler re-desired the
//! still-enabled incomplete item, and a fresh `DownloadTask` re-probed — an
//! unbounded request storm at one attempt per second per item, forever,
//! against any origin that 500s, truncates, or drops.
mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sdm_core::{AppTiming, DownloadPackage, ItemState};
use sdm_engine::{
    DownloadEngine, FakeOrigin, FakeOriginBehavior, HttpTransport, InMemoryStateStore,
    ItemSnapshot, RetryPolicy,
};
use support::{engine_settings, example_item, make_scratch_directory, test_payload, within};
use uuid::Uuid;

fn make_engine(
    transport: Arc<dyn HttpTransport>,
    folder: &Path,
    retry_policy: RetryPolicy,
) -> DownloadEngine {
    DownloadEngine::new(
        transport,
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, folder),
        retry_policy,
        None,
        None,
        None,
    )
}

fn one_package() -> (DownloadPackage, Uuid) {
    let item = example_item("a.bin");
    let id = item.id;
    (DownloadPackage::new("Batch").with_items(vec![item]), id)
}

/// Drives the heartbeat `count` times, draining whatever each tick starts.
async fn pump(engine: &DownloadEngine, count: usize) {
    for _ in 0..count {
        engine.tick().await;
        within(engine.run_until_idle()).await.unwrap();
    }
}

async fn state_of(id: Uuid, engine: &DownloadEngine) -> Option<ItemState> {
    support::snapshot_item(id, engine)
        .await
        .map(|snapshot| snapshot.state)
}

async fn item_snapshot(id: Uuid, engine: &DownloadEngine) -> Option<ItemSnapshot> {
    support::snapshot_item(id, engine).await
}

/// A permanently-500ing origin must cost a bounded number of requests, not one
/// per tick. With `max_attempts: 3` the item goes terminal after exactly three
/// attempts and issues nothing further no matter how long the engine runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transient_failures_back_off_and_become_terminal() {
    let dir = make_scratch_directory();
    let origin = Arc::new(FakeOrigin::with_behavior(
        test_payload(1000),
        FakeOriginBehavior {
            status_override: Some(500),
            ..Default::default()
        },
    ));
    let engine = make_engine(
        origin.clone(),
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(3)
            .with_base_delay(Duration::from_secs(2)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    // `add()` reconciles, so attempt 1 is already in flight.
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(state_of(id, &engine).await, Some(ItemState::Queued));
    assert_eq!(origin.requested_ranges().len(), 1);

    // Thirty heartbeats. Without backoff that is thirty requests.
    pump(&engine, 30 * AppTiming::TICKS_PER_SECOND).await;

    let final_state = state_of(id, &engine).await.expect("the item exists");
    let ItemState::Failed { reason } = final_state else {
        panic!("expected a terminal failure, got {final_state:?}");
    };
    assert!(reason.contains("3 attempts"), "{reason}");
    assert!(reason.contains("500"), "{reason}");
    assert_eq!(origin.requested_ranges().len(), 3);
}

/// Backoff must delay an item, not wedge it: once the hold expires the item is
/// re-desired, and an origin that has recovered in the meantime completes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_item_held_in_backoff_resumes_once_the_origin_recovers() {
    let dir = make_scratch_directory();
    let payload = test_payload(2000);
    let origin = Arc::new(FakeOrigin::with_behavior(
        payload.clone(),
        FakeOriginBehavior {
            truncate_after_bytes: Some(500),
            chunk_size: 100,
            ..Default::default()
        },
    ));
    let engine = make_engine(
        origin.clone(),
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(5)
            .with_base_delay(Duration::from_secs(2)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(state_of(id, &engine).await, Some(ItemState::Queued));

    // The item is in backoff right now: ticking once must not re-attempt it.
    let requests_after_first_failure = origin.requested_ranges().len();
    engine.tick().await;
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        origin.requested_ranges().len(),
        requests_after_first_failure
    );

    origin.set_behavior(FakeOriginBehavior::default());
    pump(&engine, 10 * AppTiming::TICKS_PER_SECOND).await;

    assert_eq!(state_of(id, &engine).await, Some(ItemState::Completed));
    let destination = dir.path().join("Batch").join("a.bin");
    assert_eq!(std::fs::read(destination).unwrap(), payload);
}

/// The attempt cap counts *consecutive* failures. An attempt that moved real
/// bytes before it dropped clears the counter, so a download that keeps
/// truncating but keeps progressing must finish rather than exhaust a budget
/// meant for one that cannot advance at all. Regression: `made_progress` was
/// derived from `item.completed`, which is not written back until after the
/// failure is classified, so it was always `false` and every interruption
/// counted — five truncations in a row went terminal even though each one
/// downloaded more of the file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attempts_that_make_progress_do_not_count_toward_the_cap() {
    let dir = make_scratch_directory();
    let payload = test_payload(2000);
    let origin = Arc::new(FakeOrigin::with_behavior(
        payload.clone(),
        FakeOriginBehavior {
            truncate_after_bytes: Some(400),
            chunk_size: 100,
            ..Default::default()
        },
    ));
    let engine = make_engine(
        origin,
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(3)
            .with_base_delay(Duration::from_secs(2)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();

    // Five truncations at 400 bytes each to cover a 2000-byte file — well past
    // the `max_attempts: 3` cap. The item must still complete.
    pump(&engine, 60 * AppTiming::TICKS_PER_SECOND).await;

    assert_eq!(state_of(id, &engine).await, Some(ItemState::Completed));
    let destination = dir.path().join("Batch").join("a.bin");
    assert_eq!(std::fs::read(destination).unwrap(), payload);
}

/// A transient failure that returns the item to `Queued` must not look
/// identical to a fresh queue: the snapshot carries the last error, the
/// consecutive-attempt count, and how long the backoff hold has left — and all
/// three clear once the origin recovers and the item completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transient_failure_surfaces_the_last_error_and_retry_hold_in_the_snapshot() {
    let dir = make_scratch_directory();
    let origin = Arc::new(FakeOrigin::with_behavior(
        test_payload(2000),
        FakeOriginBehavior {
            truncate_after_bytes: Some(500),
            chunk_size: 100,
            ..Default::default()
        },
    ));
    let engine = make_engine(
        origin.clone(),
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(5)
            .with_base_delay(Duration::from_secs(2)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();

    let held = item_snapshot(id, &engine).await.expect("the item exists");
    assert_eq!(held.state, ItemState::Queued);
    assert!(held.is_retrying());
    assert_eq!(held.failed_attempt_count, Some(1));
    assert!(held.last_failure_reason.is_some());
    assert!(held.retry_hold_seconds.unwrap_or(0) > 0);

    origin.set_behavior(FakeOriginBehavior::default());
    pump(&engine, 10 * AppTiming::TICKS_PER_SECOND).await;

    let done = item_snapshot(id, &engine).await.expect("the item exists");
    assert_eq!(done.state, ItemState::Completed);
    assert!(!done.is_retrying());
    assert_eq!(done.last_failure_reason, None);
    assert_eq!(done.retry_hold_seconds, None);
    assert_eq!(done.failed_attempt_count, None);
}

/// `DownloadTask::finalize()` refuses to overwrite an existing destination,
/// and that refusal is right. The engine used to classify the resulting error
/// as transient, so the item returned to `Queued`, resumed to "complete"
/// instantly from its sidecar on the next tick, and failed at the same rename
/// again — forever. Adding the same URL twice reached this immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_existing_destination_fails_permanently_instead_of_looping() {
    let dir = make_scratch_directory();
    let folder = dir.path().join("Batch");
    std::fs::create_dir_all(&folder).unwrap();
    let existing = b"do not clobber me".to_vec();
    std::fs::write(folder.join("a.bin"), &existing).unwrap();

    let origin = Arc::new(FakeOrigin::new(test_payload(1000)));
    let engine = make_engine(
        origin.clone(),
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(5)
            .with_base_delay(Duration::from_secs(1)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();

    let state = state_of(id, &engine).await.expect("the item exists");
    let ItemState::Failed { reason } = state else {
        panic!("expected a terminal failure, got {state:?}");
    };
    assert!(reason.contains("already exists"), "{reason}");

    let requests_at_failure = origin.requested_ranges().len();
    pump(&engine, 20 * AppTiming::TICKS_PER_SECOND).await;
    // Terminal means terminal: not one further request, and the user's file is
    // untouched.
    assert_eq!(origin.requested_ranges().len(), requests_at_failure);
    assert_eq!(std::fs::read(folder.join("a.bin")).unwrap(), existing);
}

/// The package folder used to be created with its error ignored. When that
/// failed, `SparseFile` failed to open, the failure classified transient, and
/// the item entered the same hot loop with nothing anywhere saying why. A file
/// sitting where the folder should be is the cheapest way to reproduce it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_uncreatable_destination_folder_is_reported_not_retried() {
    let dir = make_scratch_directory();
    // "Batch" is a regular file, so creating the directory cannot succeed.
    std::fs::write(dir.path().join("Batch"), b"blocked").unwrap();

    let origin = Arc::new(FakeOrigin::new(test_payload(1000)));
    let engine = make_engine(
        origin.clone(),
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(5)
            .with_base_delay(Duration::from_secs(1)),
    );

    let (package, id) = one_package();
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();

    let state = state_of(id, &engine).await.expect("the item exists");
    let ItemState::Failed { reason } = state else {
        panic!("expected a terminal failure, got {state:?}");
    };
    assert!(reason.contains("Could not create the folder"), "{reason}");
    assert!(reason.contains("Batch"), "{reason}");
    // Never even reached the network, and never retries.
    assert!(origin.requested_ranges().is_empty());
    pump(&engine, 10 * AppTiming::TICKS_PER_SECOND).await;
    assert!(origin.requested_ranges().is_empty());
}
