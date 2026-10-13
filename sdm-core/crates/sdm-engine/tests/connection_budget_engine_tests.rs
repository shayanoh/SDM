//! Port of Swift's `ConnectionBudgetEngineTests.swift` (2 tests).
mod support;

use sdm_core::DownloadPackage;
use sdm_engine::EngineSettings;
use support::{
    WorkerGate, WorkerGatedOrigin, example_item, make_scratch_directory, new_engine, snapshot_item,
    test_payload, wait_until_async, within,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconcile_shrinks_the_largest_worker_pool_first_under_the_global_connection_cap() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(200_000), gate.clone()),
        EngineSettings::new(2, 1, 3, dir.path().to_path_buf())
            .with_max_connections_per_host(100)
            .with_min_segment_size_bytes(64),
    );

    let item_a = example_item("a.bin");
    let item_b = example_item("b.bin");
    engine.set_segment_count(4, item_a.id).await;
    engine.set_segment_count(1, item_b.id).await;
    let (id_a, id_b) = (item_a.id, item_b.id);
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item_a, item_b]))
        .await;

    wait_until_async(|| async {
        let a_settled = snapshot_item(id_a, &engine)
            .await
            .is_some_and(|item| item.active_segments == 2);
        let b_settled = snapshot_item(id_b, &engine)
            .await
            .is_some_and(|item| item.active_segments == 1);
        a_settled && b_settled
    })
    .await;
    assert_eq!(
        snapshot_item(id_a, &engine).await.unwrap().active_segments,
        2
    );
    assert_eq!(
        snapshot_item(id_b, &engine).await.unwrap().active_segments,
        1
    );

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_host_cap_applies_even_when_the_global_cap_has_room() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(200_000), gate.clone()),
        EngineSettings::new(1, 1, 32, dir.path().to_path_buf())
            .with_max_connections_per_host(2)
            .with_min_segment_size_bytes(64),
    );

    let item = example_item("a.bin");
    let id = item.id;
    engine.set_segment_count(4, id).await;
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item]))
        .await;

    wait_until_async(|| async {
        snapshot_item(id, &engine)
            .await
            .is_some_and(|item| item.active_segments == 2)
    })
    .await;

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}
