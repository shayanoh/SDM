//! Port of Swift's `UpdateSettingsTests.swift` (1 test).
mod support;

use sdm_core::DownloadPackage;
use support::{
    WorkerGate, WorkerGatedOrigin, engine_settings, example_item, make_scratch_directory,
    new_engine, running_count, test_payload, within,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn update_settings_applies_the_new_max_concurrent_on_its_next_reconcile() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(4000), gate.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );
    let items = (0..2)
        .map(|index| example_item(&format!("{index}.bin")))
        .collect();
    engine
        .add(DownloadPackage::new("Batch").with_items(items))
        .await;

    assert_eq!(running_count(&engine).await, 1);

    engine
        .update_settings(engine_settings(2, 1, 8, dir.path()))
        .await;

    assert_eq!(running_count(&engine).await, 2);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}
