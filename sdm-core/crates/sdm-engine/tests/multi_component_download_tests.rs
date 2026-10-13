//! Port of Swift's `MultiComponentDownloadTests.swift` (7 tests; the
//! `@Test(arguments:)` churn test is one Rust test looping over its seeds).
mod support;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use sdm_core::{
    Assembly, ByteRange, DownloadItem, DownloadPackage, FileComponent, ItemState, SourceUrl,
};
use sdm_engine::{
    DownloadEngine, FakeOrigin, FakeOriginBehavior, HttpTransport, InMemoryStateStore,
    RangeRequest, RangeResponse, ResumeSidecar, RetryPolicy, SparseFile, TransportError,
};
use support::{
    FakeMuxer, RoutingTransport, ScratchTrash, SeededGenerator, WorkerGate, WorkerGatedOrigin,
    engine_settings, make_scratch_directory, snapshot_item, test_payload, wait_until_async, within,
};

fn source(string: &str) -> SourceUrl {
    SourceUrl::new(string).unwrap()
}

fn two_component_item(video: &str, audio: &str) -> DownloadItem {
    DownloadItem::new(
        vec![
            FileComponent::new(source(video), "clip.f137.mp4"),
            FileComponent::new(source(audio), "clip.f251.webm"),
        ],
        "clip.mp4",
    )
}

fn router(video: Vec<u8>, audio: Vec<u8>) -> RoutingTransport {
    RoutingTransport::new(
        "v.example",
        video,
        FakeOriginBehavior::default(),
        "a.example",
        audio,
        FakeOriginBehavior::default(),
    )
}

fn engine_over(
    transport: impl HttpTransport + 'static,
    dir: &Path,
    segments: usize,
    connections: usize,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(transport),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, segments, connections, dir),
        RetryPolicy::new(),
        None,
        None,
        None,
    )
}

fn package(items: Vec<DownloadItem>) -> DownloadPackage {
    DownloadPackage::new("P").with_items(items)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_components_download_in_parallel_and_both_parts_match_their_sources() {
    let dir = make_scratch_directory();
    let video_payload = test_payload(6000);
    let audio_payload: Vec<u8> = test_payload(1500).iter().map(|byte| byte ^ 0x5A).collect();
    let engine = engine_over(
        router(video_payload.clone(), audio_payload.clone()),
        dir.path(),
        4,
        16,
    );
    let item = two_component_item("https://v.example/f137", "https://a.example/f251");
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
    assert_eq!(
        std::fs::read(dir.path().join("P/clip.f137.mp4")).unwrap(),
        video_payload
    );
    assert_eq!(
        std::fs::read(dir.path().join("P/clip.f251.webm")).unwrap(),
        audio_payload
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_component_permanently_failing_fails_the_whole_item() {
    let dir = make_scratch_directory();
    let transport = RoutingTransport::new(
        "v.example",
        test_payload(6000),
        FakeOriginBehavior::default(),
        "a.example",
        test_payload(1500),
        FakeOriginBehavior {
            status_override: Some(404),
            ..Default::default()
        },
    );
    let engine = engine_over(transport, dir.path(), 4, 16);
    let item = two_component_item("https://v.example/f137", "https://a.example/f251");
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert!(
        matches!(snapshot.state, ItemState::Failed { .. }),
        "expected Failed, got {:?}",
        snapshot.state
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concatenated_progress_stays_well_formed_under_churn() {
    for seed in [1u64, 7, 42, 128, 999] {
        let dir = make_scratch_directory();
        let engine = engine_over(
            router(test_payload(20_000), test_payload(4000)),
            dir.path(),
            8,
            32,
        );
        let item = DownloadItem::new(
            vec![
                FileComponent::new(source("https://v.example/v"), "c.f137.mp4"),
                FileComponent::new(source("https://a.example/a"), "c.f251.webm"),
            ],
            "c.mp4",
        );
        let id = item.id;
        engine.add(package(vec![item])).await;

        let mut rng = SeededGenerator::new(seed);
        for _ in 0..40 {
            engine.tick().await;
            if rng.next_u64().is_multiple_of(2) {
                engine
                    .update_settings(engine_settings(
                        1,
                        rng.in_range(1, 8) as usize,
                        32,
                        dir.path(),
                    ))
                    .await;
            }
            let snapshot = snapshot_item(id, &engine).await.unwrap();
            let mut previous_end: i64 = -1;
            for range in snapshot.completed.ranges() {
                assert!(range.start >= 0, "seed {seed}");
                // Sorted, disjoint, coalesced.
                assert!(range.start > previous_end, "seed {seed}");
                assert!(range.end > range.start, "seed {seed}");
                previous_end = range.end;
                if let Some(total) = snapshot.total_bytes {
                    assert!(range.end <= total, "seed {seed}");
                }
            }
        }
        within(engine.run_until_idle()).await.unwrap();
        let snapshot = snapshot_item(id, &engine).await.unwrap();
        assert_eq!(snapshot.state, ItemState::Completed, "seed {seed}");
        assert_eq!(snapshot.completed.total_bytes(), 24_000, "seed {seed}");
    }
}

/// Routes the video host to a gated origin (body withheld until the gate
/// opens) and the audio host to a normal one, so we can freeze the snapshot
/// with audio complete and video at zero.
struct GatedVideoRouter {
    video: WorkerGatedOrigin,
    audio: FakeOrigin,
}

#[async_trait]
impl HttpTransport for GatedVideoRouter {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        if request.url.host_str() == Some("v.example") {
            self.video.fetch(request).await
        } else {
            self.audio.fetch(request).await
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concatenated_snapshot_accounts_for_both_components_not_just_the_larger() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let transport = GatedVideoRouter {
        video: WorkerGatedOrigin::new(test_payload(10_000), gate.clone()),
        audio: FakeOrigin::new(test_payload(2_000)),
    };
    let engine = engine_over(transport, dir.path(), 2, 8);
    let item = two_component_item("https://v.example/v", "https://a.example/a");
    let id = item.id;
    engine.add(package(vec![item])).await;

    // Audio (ungated) finishes; video is stuck at zero behind the gate.
    wait_until_async(|| async {
        snapshot_item(id, &engine)
            .await
            .is_some_and(|snapshot| snapshot.completed.total_bytes() >= 2_000)
    })
    .await;
    let mid = snapshot_item(id, &engine).await.unwrap();
    // Total is the sum; progress reflects the finished audio (2000), not
    // capped at some single component's size or lost to a base-0 overlap.
    assert_eq!(mid.total_bytes, Some(12_000));
    assert_eq!(mid.completed.total_bytes(), 2_000);
    // Audio's bytes are placed after the video span, not at offset 0.
    assert_eq!(mid.completed.ranges(), &[ByteRange::new(10_000, 12_000)]);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
    let done = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(done.completed.total_bytes(), 12_000);
    assert_eq!(done.fraction_completed(), 1.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resetting_an_incomplete_mux_download_clears_every_component_part_and_sidecar() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let transport = GatedVideoRouter {
        video: WorkerGatedOrigin::new(test_payload(20_000), gate.clone()),
        audio: FakeOrigin::new(test_payload(3_000)),
    };
    let engine = DownloadEngine::new(
        Arc::new(transport),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 2, 8, dir.path()).with_checkpoint_interval_bytes(200),
        RetryPolicy::new(),
        None,
        Some(Arc::new(FakeMuxer::new())),
        None,
    )
    .with_trash(Arc::new(ScratchTrash::new()));
    let item = DownloadItem::new(
        vec![
            FileComponent::new(source("https://v.example/v"), "clip.f137.mp4"),
            FileComponent::new(source("https://a.example/a"), "clip.f251.webm"),
        ],
        "clip.mp4",
    )
    .with_assembly(Assembly::Mux);
    let id = item.id;
    engine.add(package(vec![item])).await;

    // Wait for the audio component to finish (its part file finalizes) and
    // video to have a sidecar, then tick so checkpoints land.
    let audio_final = dir.path().join("P/clip.f251.webm");
    wait_until_async(|| async { audio_final.exists() }).await;
    for _ in 0..3 {
        engine.tick().await;
    }
    let video_incomplete = SparseFile::incomplete_path(&dir.path().join("P/clip.f137.mp4"));
    assert!(video_incomplete.exists());

    engine.reset_download(id).await;

    // Everything the item wrote is gone.
    for name in ["clip.mp4", "clip.f137.mp4", "clip.f251.webm"] {
        let base = dir.path().join("P").join(name);
        assert!(!base.exists(), "{name}");
        assert!(!SparseFile::incomplete_path(&base).exists(), "{name}");
        assert!(!ResumeSidecar::path_for(&base).exists(), "{name}");
    }

    // And it resumes cleanly to a hash-matching pair of parts.
    gate.open();
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_exposes_every_component_part_filename() {
    let dir = make_scratch_directory();
    let engine = engine_over(
        router(test_payload(1000), test_payload(300)),
        dir.path(),
        2,
        8,
    );
    let multi = two_component_item("https://v.example/v", "https://a.example/a");
    let plain = DownloadItem::http(source("https://x/f.bin"), "f.bin");
    let (multi_id, plain_id) = (multi.id, plain.id);
    engine.add(package(vec![multi, plain])).await;
    assert_eq!(
        snapshot_item(multi_id, &engine)
            .await
            .unwrap()
            .part_filenames,
        ["clip.f137.mp4", "clip.f251.webm"]
    );
    assert_eq!(
        snapshot_item(plain_id, &engine)
            .await
            .unwrap()
            .part_filenames,
        ["f.bin"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_single_component_item_whose_part_name_differs_is_renamed_to_the_output_name() {
    let dir = make_scratch_directory();
    let payload = test_payload(3000);
    let engine = engine_over(FakeOrigin::new(payload.clone()), dir.path(), 2, 8);
    // A stale-shaped progressive item: part file "clip.f18.mp4", output
    // "clip.mp4", assembly none.
    let component = FileComponent::new(source("https://gv/prog"), "clip.f18.mp4").with_origin(
        sdm_core::ComponentOrigin::Resolved {
            format_id: "18".to_string(),
        },
    );
    let item = DownloadItem::new(vec![component], "clip.mp4");
    let id = item.id;
    engine.add(package(vec![item])).await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
    assert!(!snapshot.file_missing);
    assert_eq!(
        std::fs::read(dir.path().join("P/clip.mp4")).unwrap(),
        payload
    );
    assert!(!dir.path().join("P/clip.f18.mp4").exists());
}
