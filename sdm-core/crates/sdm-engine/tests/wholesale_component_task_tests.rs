//! Rust-only tests for `WholesaleComponentTask` (the Swift suite reaches it
//! only through the engine). They pin the progress synthesis, the resume
//! flag and the scratch-cleanup rules.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sdm_core::{ByteRange, RangeSet, SourceUrl, WholesaleError, WholesalePhase, WholesaleProgress};
use sdm_engine::WholesaleComponentTask;
use support::{FakeWholesaleDownloader, make_scratch_directory, within};
use uuid::Uuid;

fn new_task(
    destination: &Path,
    downloader: &Arc<FakeWholesaleDownloader>,
) -> Arc<WholesaleComponentTask> {
    Arc::new(WholesaleComponentTask::new(
        Uuid::new_v4(),
        SourceUrl::new("https://example.com/watch?v=1").expect("a valid URL"),
        "bv*+ba/b",
        destination.to_path_buf(),
        downloader.clone(),
    ))
}

fn bytes(downloaded: i64) -> WholesaleProgress {
    WholesaleProgress {
        downloaded_bytes: Some(downloaded),
        ..Default::default()
    }
}

fn touch(path: PathBuf) {
    std::fs::write(path, b"x").expect("write a scratch file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_is_synthesized_as_one_contiguous_range() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("Clip.mp4");
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let task = new_task(&destination, &downloader);
    assert_eq!(task.probed_supports_ranges(), Some(false));
    assert_eq!(task.active_worker_count(), 0);

    let running = tokio::spawn({
        let task = task.clone();
        async move { task.start().await }
    });
    within(downloader.wait_until_started()).await;

    // A fragment ratio re-derives the total from the bytes so far.
    downloader.emit(WholesaleProgress {
        downloaded_bytes: Some(250),
        fraction: Some(0.25),
        is_fragmented: true,
        ..Default::default()
    });
    assert_eq!(task.expected_total_bytes(), Some(1000));
    assert_eq!(task.active_worker_count(), 1);
    assert_eq!(task.probed_supports_ranges(), Some(true));
    let mut expected = RangeSet::new();
    expected.insert(ByteRange::new(0, 250));
    assert_eq!(task.completed_ranges(), expected);

    // Downloaded bytes never go backwards, and are clamped to the total.
    downloader.emit(bytes(100));
    assert_eq!(task.completed_ranges(), expected);
    downloader.emit(WholesaleProgress {
        downloaded_bytes: Some(2000),
        total_bytes: Some(1500),
        ..Default::default()
    });
    assert_eq!(task.expected_total_bytes(), Some(1500));
    let mut clamped = RangeSet::new();
    clamped.insert(ByteRange::new(0, 1500));
    assert_eq!(task.completed_ranges(), clamped);

    downloader.emit(WholesaleProgress {
        phase: WholesalePhase::PostProcessing,
        ..Default::default()
    });
    assert!(task.is_assembling());
    assert!(task.telemetry().is_assembling);

    downloader.finish_success();
    let output = within(running).await.expect("join").expect("succeeds");
    assert_eq!(output, destination);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_waits_for_the_downloader_and_keeps_scratch_and_progress() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("Clip.mp4");
    touch(dir.path().join("Clip.mp4.part"));
    touch(dir.path().join("Clip.mp4.ytdl"));
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let task = new_task(&destination, &downloader);

    let running = tokio::spawn({
        let task = task.clone();
        async move { task.start().await }
    });
    within(downloader.wait_until_started()).await;
    downloader.emit(bytes(300));

    within(task.pause()).await;
    // `pause` returned, so `start` has nothing left to wait on.
    let result = within(running).await.expect("join");
    assert_eq!(result, Err(WholesaleError::Cancelled));
    assert!(dir.path().join("Clip.mp4.part").exists());
    assert!(dir.path().join("Clip.mp4.ytdl").exists());
    let mut kept = RangeSet::new();
    kept.insert(ByteRange::new(0, 300));
    assert_eq!(task.completed_ranges(), kept);

    // A paused task is finished: it never calls the downloader again.
    assert_eq!(within(task.start()).await, Err(WholesaleError::Cancelled));
    assert_eq!(downloader.call_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failure_sweeps_scratch_and_resets_progress() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("Clip.mp4");
    touch(destination.clone());
    touch(dir.path().join("Clip.mp4.part"));
    touch(dir.path().join("Clip.f137.mp4"));
    touch(dir.path().join("Clip.final.mp4"));
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let task = new_task(&destination, &downloader);

    let running = tokio::spawn({
        let task = task.clone();
        async move { task.start().await }
    });
    within(downloader.wait_until_started()).await;
    downloader.emit(WholesaleProgress {
        downloaded_bytes: Some(10),
        is_fragmented: true,
        ..Default::default()
    });
    downloader.finish_failure(WholesaleError::Unavailable);

    let result = within(running).await.expect("join");
    assert_eq!(result, Err(WholesaleError::Unavailable));
    assert!(!destination.exists());
    assert!(!dir.path().join("Clip.mp4.part").exists());
    assert!(!dir.path().join("Clip.f137.mp4").exists());
    assert!(dir.path().join("Clip.final.mp4").exists());
    assert_eq!(task.completed_ranges(), RangeSet::new());
    assert_eq!(task.expected_total_bytes(), None);
    assert_eq!(task.active_worker_count(), 0);
    assert_eq!(task.probed_supports_ranges(), Some(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn success_keeps_the_output_and_sweeps_only_scratch() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("Clip.mp4");
    touch(dir.path().join("Clip.mp4.ytdl"));
    touch(dir.path().join("Clip.mp4-Frag3"));
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let task = new_task(&destination, &downloader);

    let running = tokio::spawn({
        let task = task.clone();
        async move { task.start().await }
    });
    within(downloader.wait_until_started()).await;
    downloader.finish_success();

    let output = within(running).await.expect("join").expect("succeeds");
    assert_eq!(output, destination);
    assert!(destination.exists());
    assert!(!dir.path().join("Clip.mp4.ytdl").exists());
    assert!(!dir.path().join("Clip.mp4-Frag3").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn success_without_an_output_file_is_a_failure() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("Clip.mp4");
    let downloader = Arc::new(FakeWholesaleDownloader::new());
    let task = new_task(&destination, &downloader);

    let running = tokio::spawn({
        let task = task.clone();
        async move { task.start().await }
    });
    within(downloader.wait_until_started()).await;
    // Completes the call without the fake writing the output.
    std::fs::remove_file(&destination).ok();
    downloader.finish_without_output();
    let result = within(running).await.expect("join");
    assert_eq!(
        result,
        Err(WholesaleError::Failed {
            stderr_tail: "yt-dlp produced no output file".to_string()
        })
    );
}
