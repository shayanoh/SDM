//! Port of Swift's `MuxAssemblyTests.swift` (4 tests). `FakeMuxer` lives in
//! `support/fake_muxer.rs`.
mod support;

use std::sync::Arc;

use sdm_core::{Assembly, DownloadItem, DownloadPackage, FileComponent, ItemState, SourceUrl};
use sdm_engine::{DownloadEngine, FakeOriginBehavior, InMemoryStateStore, MuxError, RetryPolicy};
use support::{
    FakeMuxer, RoutingTransport, engine_settings, make_scratch_directory, snapshot_item,
    test_payload, within,
};

fn mux_item(video: &str, audio: &str) -> DownloadItem {
    DownloadItem::new(
        vec![
            FileComponent::new(SourceUrl::new(video).unwrap(), "clip.f137.mp4"),
            FileComponent::new(SourceUrl::new(audio).unwrap(), "clip.f251.webm"),
        ],
        "clip.mp4",
    )
    .with_assembly(Assembly::Mux)
}

fn router(video: usize, audio: usize) -> RoutingTransport {
    RoutingTransport::new(
        "v.example",
        test_payload(video),
        FakeOriginBehavior::default(),
        "a.example",
        test_payload(audio),
        FakeOriginBehavior::default(),
    )
}

fn engine_with(
    dir: &std::path::Path,
    segments: usize,
    connections: usize,
    router: RoutingTransport,
    muxer: Option<Arc<FakeMuxer>>,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(router),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, segments, connections, dir),
        RetryPolicy::new(),
        None,
        muxer.map(|muxer| muxer as Arc<dyn sdm_engine::Muxer>),
        None,
    )
}

fn package(item: DownloadItem) -> DownloadPackage {
    DownloadPackage::new("P").with_items(vec![item])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_runs_after_both_components_complete_then_deletes_parts() {
    let dir = make_scratch_directory();
    let muxer = Arc::new(FakeMuxer::new());
    let engine = engine_with(dir.path(), 4, 16, router(4000, 1000), Some(muxer.clone()));
    let item = mux_item("https://v.example/v", "https://a.example/a");
    let id = item.id;
    engine.add(package(item)).await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
    assert_eq!(muxer.calls().len(), 1);
    assert!(dir.path().join("P/clip.mp4").exists());
    assert!(!dir.path().join("P/clip.f137.mp4").exists());
    assert!(!dir.path().join("P/clip.f251.webm").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_failure_fails_the_item_keeps_parts_and_can_retry() {
    let dir = make_scratch_directory();
    let muxer = Arc::new(FakeMuxer::new());
    muxer.set_behavior(|_| {
        Err(MuxError::FfmpegFailed {
            stderr_tail: "Invalid data".to_string(),
        })
    });
    let engine = engine_with(dir.path(), 4, 16, router(4000, 1000), Some(muxer.clone()));
    let item = mux_item("https://v.example/v", "https://a.example/a");
    let id = item.id;
    engine.add(package(item)).await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    let ItemState::Failed { reason } = snapshot.state else {
        panic!("expected Failed, got {:?}", snapshot.state);
    };
    assert!(reason.contains("mux"), "{reason}");
    assert!(dir.path().join("P/clip.f137.mp4").exists());

    muxer.set_behavior(|output| {
        std::fs::write(output, b"MUXED").unwrap();
        Ok(())
    });
    engine.retry_mux(id).await;
    let after = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(after.state, ItemState::Completed);
    assert_eq!(muxer.calls().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_item_without_a_muxer_fails_with_a_clear_reason() {
    let dir = make_scratch_directory();
    let engine = engine_with(dir.path(), 4, 16, router(4000, 1000), None);
    let item = mux_item("https://v.example/v", "https://a.example/a");
    let id = item.id;
    engine.add(package(item)).await;
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert!(
        matches!(snapshot.state, ItemState::Failed { .. }),
        "expected Failed, got {:?}",
        snapshot.state
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_carries_the_items_assembly_mode() {
    let dir = make_scratch_directory();
    let engine = engine_with(
        dir.path(),
        2,
        8,
        router(2000, 500),
        Some(Arc::new(FakeMuxer::new())),
    );
    let item = mux_item("https://v.example/v", "https://a.example/a");
    let id = item.id;
    engine.add(package(item)).await;
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.assembly, Assembly::Mux);
}
