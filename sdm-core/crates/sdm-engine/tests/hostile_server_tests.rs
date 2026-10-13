mod support;

use std::sync::Arc;

use sdm_engine::{DownloadError, DownloadTask, FakeOrigin, FakeOriginBehavior, ResumeSidecar};
use support::{make_scratch_directory, test_configuration, test_payload, test_source, within};
use uuid::Uuid;

fn new_task(
    destination: std::path::PathBuf,
    payload: Vec<u8>,
    behavior: FakeOriginBehavior,
    workers: usize,
) -> DownloadTask {
    DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination,
        Arc::new(FakeOrigin::with_behavior(payload, behavior)),
        test_configuration(workers, 64),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_ignoring_ranges_forces_single_worker() {
    let dir = make_scratch_directory();
    let payload = test_payload(5000);
    let behavior = FakeOriginBehavior {
        ignores_ranges: true,
        ..Default::default()
    };
    let task = new_task(dir.path().join("out.bin"), payload.clone(), behavior, 16);

    let result = within(task.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
    assert!(!task.supports_ranges());
    // The clamp itself: without it, 16 workers would spawn, one would
    // reserve the whole gap, and the other 15 would simply get `None` from
    // `claim_next` and return — byte identity and `supports_ranges` alone
    // don't prove the pool was ever actually held to 1.
    assert_eq!(task.peak_worker_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_overstating_size_fails_rather_than_truncating() {
    let dir = make_scratch_directory();
    let behavior = FakeOriginBehavior {
        reported_size_override: Some(9999),
        ..Default::default()
    };
    let task = new_task(dir.path().join("out.bin"), test_payload(1000), behavior, 1);

    // `reported_size_override` makes the probe claim 9999 bytes while the
    // actual payload is 1000. `next_claim` no longer halves — the single
    // worker's first (and only) claim is the whole reported gap, 0..9999;
    // FakeOrigin clips the slice to the real payload and ends the stream
    // cleanly at byte 1000 — a clean short read, which `download`
    // reports precisely as `TruncatedResponse`.
    let error = within(task.start()).await.unwrap_err();
    assert_eq!(
        error,
        DownloadError::TruncatedResponse {
            expected: 9999,
            received: 1000
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mid_body_drop_leaves_recoverable_partial_progress() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(500),
        chunk_size: 64,
        ..Default::default()
    };
    let task = new_task(destination.clone(), test_payload(4000), behavior, 1);

    let _ = within(task.start()).await;
    task.pause();

    let sidecar = ResumeSidecar::load(&ResumeSidecar::path_for(&destination));
    assert!(sidecar.is_some());
    assert!(sidecar.unwrap().completed.total_bytes() >= 448);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn partial_file_is_never_renamed_into_place() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(200),
        ..Default::default()
    };
    let task = new_task(destination.clone(), test_payload(4000), behavior, 1);

    let _ = within(task.start()).await;
    assert!(!destination.exists());
}

/// Regression test for Finding 1 (round 1 review): a `Range`-ignoring
/// origin cannot be resumed. If an interrupted run against such an origin
/// were allowed to checkpoint a sidecar, a second run against the same
/// destination would trust that sidecar's non-zero `completed` set, hand
/// `claim_next` a non-zero-start claim, and then splice a clean origin's
/// from-byte-0 body into the middle of the file — a silent, unflagged
/// corruption that still finalizes successfully.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ignores_ranges_resume_never_produces_corrupt_file() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(5000);

    let drop_behavior = FakeOriginBehavior {
        ignores_ranges: true,
        drop_after_bytes: Some(500),
        chunk_size: 64,
        ..Default::default()
    };
    let first = new_task(destination.clone(), payload.clone(), drop_behavior, 1);
    let _ = within(first.start()).await;
    first.pause();

    let clean_behavior = FakeOriginBehavior {
        ignores_ranges: true,
        ..Default::default()
    };
    let second = new_task(destination.clone(), payload.clone(), clean_behavior, 1);
    let result = within(second.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}
