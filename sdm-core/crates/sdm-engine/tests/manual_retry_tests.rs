//! Port of Swift's `ManualRetryTests.swift` (4 tests).
mod support;

use std::sync::Arc;
use std::time::Duration;

use sdm_core::{DownloadPackage, ItemState};
use sdm_engine::{DownloadEngine, FakeOrigin, FakeOriginBehavior, InMemoryStateStore, RetryPolicy};
use support::{
    engine_settings, example_item, make_scratch_directory, snapshot_item, test_payload, within,
};

fn engine_over(
    origin: Arc<FakeOrigin>,
    dir: &std::path::Path,
    policy: RetryPolicy,
) -> DownloadEngine {
    DownloadEngine::new(
        origin,
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, dir),
        policy,
        None,
        None,
        None,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remaining_attempts_is_none_when_an_item_has_never_failed() {
    let dir = make_scratch_directory();
    let engine = engine_over(
        Arc::new(FakeOrigin::new(test_payload(1000))),
        dir.path(),
        RetryPolicy::new(),
    );
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().remaining_attempts,
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remaining_attempts_counts_down_after_a_transient_failure() {
    let dir = make_scratch_directory();
    let origin = Arc::new(FakeOrigin::with_behavior(
        test_payload(1000),
        FakeOriginBehavior {
            status_override: Some(500),
            ..Default::default()
        },
    ));
    let engine = engine_over(
        origin,
        dir.path(),
        RetryPolicy::new()
            .with_max_attempts(3)
            .with_base_delay(Duration::from_secs(30)),
    );
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.remaining_attempts, Some(2));
    assert_eq!(snapshot.state, ItemState::Queued);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_on_a_failed_item_resumes_it_after_the_origin_recovers() {
    let dir = make_scratch_directory();
    let origin = Arc::new(FakeOrigin::with_behavior(
        test_payload(1000),
        FakeOriginBehavior {
            status_override: Some(404),
            ..Default::default()
        },
    ));
    let engine = engine_over(origin.clone(), dir.path(), RetryPolicy::new());
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    assert!(
        matches!(
            snapshot_item(id, &engine).await.unwrap().state,
            ItemState::Failed { .. }
        ),
        "expected the item to be terminally failed after a 404"
    );

    origin.set_behavior(FakeOriginBehavior::default());
    engine.retry(id).await;
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
    assert_eq!(
        std::fs::read(dir.path().join("Batch").join("a.bin")).unwrap(),
        test_payload(1000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_on_any_non_failed_item_is_a_no_op() {
    let dir = make_scratch_directory();
    let engine = engine_over(
        Arc::new(FakeOrigin::new(test_payload(1000))),
        dir.path(),
        RetryPolicy::new(),
    );
    let item = example_item("a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    engine.retry(id).await;
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}
