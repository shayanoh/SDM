//! Stopping, removing or shutting down while a download is still in its
//! first (probe) request returns at once; the request is abandoned, not
//! waited out. The origin's probe never answers (the gate is never opened),
//! so a wait would only end at the test's own bound.
mod support;

use std::time::Duration;

use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use sdm_engine::{DownloadEngine, RetryPolicy};
use support::{
    Gate, GatePhase, make_gated_engine, make_scratch_directory, snapshot_item, test_payload,
    test_source,
};
use uuid::Uuid;

/// Long enough for a loaded machine, far shorter than the transport timeouts
/// a stuck probe would otherwise wait out.
const PROMPT: Duration = Duration::from_secs(10);

async fn prompt<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(PROMPT, future)
        .await
        .unwrap_or_else(|_| panic!("{what} did not return while the probe was parked"))
}

/// An engine whose only item is parked inside its probe request.
async fn engine_with_parked_probe(dir: &std::path::Path) -> (DownloadEngine, Uuid, Gate) {
    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(1000),
        dir,
        &gate,
        GatePhase::Probe,
        false,
        RetryPolicy::new(),
    );
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;
    prompt("the probe arriving", gate.wait_for_arrival()).await;
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Running
    );
    (engine, id, gate)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_item_returns_while_the_probe_is_parked() {
    let dir = make_scratch_directory();
    let (engine, id, _gate) = engine_with_parked_probe(dir.path()).await;

    prompt("stop_item", engine.stop_item(id)).await;

    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Stopped
    );
    prompt("shutdown", engine.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_item_returns_while_the_probe_is_parked() {
    let dir = make_scratch_directory();
    let (engine, id, _gate) = engine_with_parked_probe(dir.path()).await;

    prompt("remove_item", engine.remove_item(id, false)).await;

    assert!(snapshot_item(id, &engine).await.is_none());
    prompt("shutdown", engine.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_returns_while_the_probe_is_parked() {
    let dir = make_scratch_directory();
    let (engine, id, _gate) = engine_with_parked_probe(dir.path()).await;

    prompt("shutdown", engine.shutdown()).await;

    // The interrupted attempt leaves the item stopped, not failed.
    let state = snapshot_item(id, &engine).await.unwrap().state;
    assert!(
        matches!(state, ItemState::Stopped | ItemState::Queued),
        "{state:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_probe_leaves_the_engine_usable() {
    let dir = make_scratch_directory();
    let (engine, id, gate) = engine_with_parked_probe(dir.path()).await;
    prompt("stop_item", engine.stop_item(id)).await;

    gate.open();
    engine.start_item(id).await;
    prompt("run_until_idle", engine.run_until_idle())
        .await
        .unwrap();
    assert_eq!(
        snapshot_item(id, &engine).await.unwrap().state,
        ItemState::Completed
    );
}
