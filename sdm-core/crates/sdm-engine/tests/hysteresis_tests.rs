//! Port of Swift's `HysteresisTests.swift` (1 test).
mod support;

use sdm_core::{AppTiming, DownloadPackage, ItemState, Priority};
use support::{
    WorkerGate, WorkerGatedOrigin, engine_settings, example_item, make_scratch_directory,
    new_engine, snapshot_item, test_payload, wait_until_async, within,
};

/// A running item that has not yet been probed (`is_resumable == None`) is not
/// protected by the scheduler's pass 1 (`Some(false)` only), so this isolates
/// hysteresis specifically: without it, a higher-priority item added a moment
/// later would preempt A instantly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recently_started_running_items_survive_a_preempting_addition_until_the_hysteresis_window_elapses()
 {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(4000), gate.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );

    let item_a = example_item("a.bin");
    let id_a = item_a.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item_a]))
        .await;

    wait_until_async(|| async {
        snapshot_item(id_a, &engine)
            .await
            .is_some_and(|item| item.state == ItemState::Running)
    })
    .await;

    let item_b = example_item("b.bin").with_priority(Priority::Highest);
    let id_b = item_b.id;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item_b]))
        .await;

    // Still inside the hysteresis window: A keeps its slot despite B outranking it.
    assert_eq!(
        snapshot_item(id_a, &engine).await.unwrap().state,
        ItemState::Running
    );
    assert_eq!(
        snapshot_item(id_b, &engine).await.unwrap().state,
        ItemState::Queued
    );

    for _ in 0..(AppTiming::TICKS_PER_SECOND * 5) {
        engine.tick().await;
    }

    // Window elapsed: B now preempts A.
    assert_eq!(
        snapshot_item(id_a, &engine).await.unwrap().state,
        ItemState::Queued
    );
    assert_eq!(
        snapshot_item(id_b, &engine).await.unwrap().state,
        ItemState::Running
    );

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}
