//! The `DownloadTask` half of Swift's `CheckpointTickTests.swift` (8 of its
//! 9 tests); the ninth drives a `DownloadEngine` and moves to Task 16.
mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use sdm_core::AppTiming;
use sdm_engine::{
    DownloadError, DownloadTask, FakeOrigin, FakeOriginBehavior, HttpTransport, RangeRequest,
    RangeResponse, ResumeSidecar, TransportError,
};
use support::{make_scratch_directory, test_configuration, test_payload, test_source, within};
use uuid::Uuid;

// Covers the time-based checkpoint trigger added alongside speed telemetry
// (spec §4.3: checkpoint "every ~8 MB per worker or every 5 s, whichever
// comes first"). `record` already implements the byte trigger;
// `checkpoint_tick()` is the engine's heartbeat driving the wall-clock one,
// so a slow or drip-feeding server can't go unbounded time without a
// checkpoint.

const DROPPED: DownloadError = DownloadError::Transport(TransportError::ConnectionDropped);

fn new_task(destination: &Path, transport: Arc<dyn HttpTransport>) -> DownloadTask {
    DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination.to_path_buf(),
        transport,
        test_configuration(1, 64),
    )
}

fn dropping_origin(
    payload_len: usize,
    drop_after_bytes: usize,
    ignores_ranges: bool,
) -> FakeOrigin {
    FakeOrigin::with_behavior(
        test_payload(payload_len),
        FakeOriginBehavior {
            drop_after_bytes: Some(drop_after_bytes),
            ignores_ranges,
            ..Default::default()
        },
    )
}

fn staleness() -> usize {
    AppTiming::TICKS_PER_SECOND * 5
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_tick_writes_sidecar_after_five_ticks() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    // drop_after_bytes: 0 fails the very first chunk, before any bytes are
    // recorded, so the byte trigger in `record` can never fire — only the
    // tick-driven trigger under test can produce a sidecar here.
    let task = new_task(&destination, Arc::new(dropping_origin(4000, 0, false)));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    let sidecar_path = ResumeSidecar::path_for(&destination);
    assert!(!sidecar_path.exists());

    for _ in 0..staleness() - 1 {
        task.checkpoint_tick();
    }
    assert!(!sidecar_path.exists());

    task.checkpoint_tick();
    assert!(sidecar_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_tick_resets_staleness_counter_after_firing() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    let task = new_task(&destination, Arc::new(dropping_origin(4000, 0, false)));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    let sidecar_path = ResumeSidecar::path_for(&destination);
    for _ in 0..staleness() {
        task.checkpoint_tick();
    }
    assert!(sidecar_path.exists());
    std::fs::remove_file(&sidecar_path).unwrap();

    // If the counter weren't reset when the checkpoint fired, it would
    // already be past threshold and the very next tick would re-fire.
    for _ in 0..staleness() - 1 {
        task.checkpoint_tick();
    }
    assert!(!sidecar_path.exists());

    task.checkpoint_tick();
    assert!(sidecar_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_from_byte_trigger_resets_tick_counter_too_so_double_write_cannot_happen() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    // min_chunk 64 / checkpoint_interval 128 (from `test_configuration`); the
    // drop at 300 bytes lands after the byte trigger has already fired at
    // least once, then kills the connection so the file stays open for the
    // tick calls below.
    let task = new_task(&destination, Arc::new(dropping_origin(4000, 300, false)));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    let sidecar_path = ResumeSidecar::path_for(&destination);
    assert!(sidecar_path.exists());
    std::fs::remove_file(&sidecar_path).unwrap();

    for _ in 0..staleness() - 1 {
        task.checkpoint_tick();
    }
    assert!(!sidecar_path.exists());

    task.checkpoint_tick();
    assert!(sidecar_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_tick_does_not_resurrect_sidecar_after_successful_completion() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    let task = new_task(&destination, Arc::new(FakeOrigin::new(test_payload(1000))));
    within(task.start()).await.unwrap();

    for _ in 0..10 {
        task.checkpoint_tick();
    }
    assert!(!ResumeSidecar::path_for(&destination).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_tick_never_writes_sidecar_for_non_resumable_download() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    let task = new_task(&destination, Arc::new(dropping_origin(4000, 0, true)));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    for _ in 0..10 {
        task.checkpoint_tick();
    }
    assert!(!ResumeSidecar::path_for(&destination).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_tick_is_a_no_op_before_start() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    let task = new_task(&destination, Arc::new(FakeOrigin::new(test_payload(1000))));
    for _ in 0..10 {
        task.checkpoint_tick();
    }
    assert!(!ResumeSidecar::path_for(&destination).exists());
}

// MARK: - Checkpoint durability and failure reporting

/// Makes the download's folder read-only at the moment the *body* request is
/// issued — i.e. after `prepare()` has already removed any stale sidecar and
/// opened the `.incomplete` file, but before the first checkpoint.
///
/// The open descriptor keeps working (`pwrite` doesn't consult directory
/// permissions), so bytes still land correctly at their absolute offsets; only
/// creating the new `.sdmpart` file fails. That is the real-world shape of the
/// bug — the download appears healthy while its resume state is silently
/// being lost.
struct FolderLockingOrigin {
    inner: FakeOrigin,
    folder: std::path::PathBuf,
}

#[async_trait]
impl HttpTransport for FolderLockingOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let range = request.range;
        let response = self.inner.fetch(request).await?;
        if range.is_some_and(|range| range.length() > 1) {
            let _ = std::fs::set_permissions(&self.folder, std::fs::Permissions::from_mode(0o500));
        }
        Ok(response)
    }
}

/// Restores write permission on drop so the scratch directory can be removed.
struct Unlock(std::path::PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// A sidecar that cannot be written means a crash loses every byte of
/// progress with nothing anywhere saying so. `checkpoint()` must record the
/// failure rather than swallow it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failing_checkpoint_is_recorded_rather_than_swallowed() {
    let dir = make_scratch_directory();
    let _unlock = Unlock(dir.path().to_path_buf());
    let destination = dir.path().join("out.bin");

    let origin = FolderLockingOrigin {
        inner: dropping_origin(4000, 300, false),
        folder: dir.path().to_path_buf(),
    };
    let task = new_task(&destination, Arc::new(origin));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    let reason = task.last_checkpoint_failure().expect("a recorded failure");
    assert!(reason.contains("out.bin"), "{reason}");
    assert!(task.checkpoint_failure_count() > 0);
    // The bytes themselves were unaffected — only the resume state was lost.
    assert_eq!(task.completed_ranges().total_bytes(), 300);
    assert!(!ResumeSidecar::path_for(&destination).exists());
}

/// The healthy path must leave no residue: a checkpoint that succeeds clears
/// any previously recorded failure, so a one-off hiccup doesn't stick to the
/// item forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_successful_checkpoint_reports_no_failure() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");

    let task = new_task(&destination, Arc::new(dropping_origin(4000, 300, false)));
    assert_eq!(within(task.start()).await.unwrap_err(), DROPPED);

    assert!(ResumeSidecar::path_for(&destination).exists());
    assert_eq!(task.last_checkpoint_failure(), None);
    assert_eq!(task.checkpoint_failure_count(), 0);
}
