mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sdm_core::{ByteRange, RangeSet};
use sdm_engine::{DownloadTask, FakeOrigin, FakeOriginBehavior, ResumeSidecar, SparseFile};
use support::{
    StallingOrigin, WorkerGate, make_scratch_directory, test_configuration, test_payload,
    test_source, wait_until, within,
};
use uuid::Uuid;

fn new_task(
    destination: &Path,
    transport: Arc<dyn sdm_engine::HttpTransport>,
    workers: usize,
    min_chunk: i64,
) -> Arc<DownloadTask> {
    Arc::new(DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination.to_path_buf(),
        transport,
        test_configuration(workers, min_chunk),
    ))
}

/// Runs a download to partial completion, then abandons the task object,
/// leaving `.incomplete` and `.sdmpart` on disk.
async fn download_partially(payload: &[u8], destination: &Path, stop_after: usize) {
    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(stop_after),
        chunk_size: 64,
        ..Default::default()
    };
    let task = new_task(
        destination,
        Arc::new(FakeOrigin::with_behavior(payload.to_vec(), behavior)),
        1,
        64,
    );
    let _ = within(task.start()).await;
    task.pause();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resuming_from_sidecar_produces_identical_file() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(8000);

    download_partially(&payload, &destination, 2000).await;
    assert!(ResumeSidecar::load(&ResumeSidecar::path_for(&destination)).is_some());

    let resumed = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        4,
        64,
    );
    let result = within(resumed.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_does_not_refetch_already_completed_bytes() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(8000);

    // Any value strictly inside [0, 8000) works now that a single worker's
    // claim is always the whole file rather than half of it; 3000 keeps a
    // healthy margin from both ends.
    download_partially(&payload, &destination, 3000).await;
    let carried = ResumeSidecar::load(&ResumeSidecar::path_for(&destination)).unwrap();
    assert!(carried.completed.total_bytes() > 0);

    let origin = FakeOrigin::new(payload.clone());
    let resumed = new_task(&destination, Arc::new(origin.clone()), 1, 64);
    within(resumed.start()).await.unwrap();

    let overlap = origin
        .requested_ranges()
        .into_iter()
        .filter(|range| range.length() > 1)
        .any(|claim| {
            carried
                .completed
                .ranges()
                .iter()
                .any(|done| done.start < claim.end && claim.start < done.end)
        });
    assert!(!overlap);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_sidecar_restarts_from_zero() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    download_partially(&payload, &destination, 1000).await;
    ResumeSidecar::remove(&ResumeSidecar::path_for(&destination));

    let resumed = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    let result = within(resumed.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_sidecar_restarts_from_zero() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    download_partially(&payload, &destination, 1000).await;
    std::fs::write(ResumeSidecar::path_for(&destination), b"garbage").unwrap();

    let resumed = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    let result = within(resumed.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changed_validator_discards_partial_and_restarts() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    let behavior = FakeOriginBehavior {
        validator: Some("etag-1".to_string()),
        drop_after_bytes: Some(1000),
        chunk_size: 64,
        ..Default::default()
    };
    let first = new_task(
        &destination,
        Arc::new(FakeOrigin::with_behavior(payload.clone(), behavior)),
        1,
        64,
    );
    let _ = within(first.start()).await;
    first.pause();

    let changed = FakeOriginBehavior {
        validator: Some("etag-2".to_string()),
        ..Default::default()
    };
    let second_origin = FakeOrigin::with_behavior(payload.clone(), changed);
    let resumed = new_task(&destination, Arc::new(second_origin.clone()), 1, 64);
    within(resumed.start()).await.unwrap();

    assert_eq!(
        resumed.completed_ranges().ranges(),
        &[ByteRange::new(0, 4000)]
    );
    assert_eq!(std::fs::read(&destination).unwrap(), payload);

    // Both origins serve the identical payload, so byte identity and the
    // coalesced completed-range assertions above would also pass under a
    // wrongly-*resumed* download that trusted the stale sidecar — they are
    // not proof the validator guard actually did anything. What can't be
    // faked: a wrong resume would only ask the second origin for the
    // remaining gap [1000, 4000); a correct restart-from-zero must genuinely
    // re-fetch the discarded prefix [0, 1000) too. Assert the second origin
    // actually saw requests covering the whole file, not just the tail.
    let mut refetched = RangeSet::new();
    for claim in second_origin
        .requested_ranges()
        .into_iter()
        .filter(|range| range.length() > 1)
    {
        refetched.insert(claim);
    }
    assert!(refetched.is_complete(4000));
}

/// Hazard: `pause()` closes the file while workers may still be mid-write.
/// If `download` recorded bytes without checking the file is still open, a
/// paused-mid-flight download could mark ranges complete in the sidecar that
/// were never actually written to disk — a Frankenstein file on resume.
/// This drives a real download to genuine concurrency (every worker has
/// written some bytes and is held in flight at a gate) before pausing, then
/// verifies every byte the sidecar claims as complete matches the source at
/// that offset.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_mid_flight_never_records_unwritten_bytes() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(50_000);

    let behavior = FakeOriginBehavior {
        chunk_size: 256,
        ..Default::default()
    };
    let gate = WorkerGate::new();
    let origin = StallingOrigin {
        inner: FakeOrigin::with_behavior(payload.clone(), behavior),
        gate: gate.clone(),
        free_items: 4,
    };
    let task = new_task(&destination, Arc::new(origin), 4, 64);

    let handle = {
        let task = task.clone();
        tokio::spawn(async move { task.start().await })
    };

    // Wait for genuine mid-flight progress: at least one byte actually
    // written and recorded, with the pool still short of the full payload —
    // the stalled origin guarantees it stays short.
    wait_until(|| task.completed_ranges().total_bytes() > 0).await;
    assert!(task.completed_ranges().total_bytes() < payload.len() as i64);

    task.pause();
    // The stalled workers are released by the pause itself, never by the gate.
    let outcome = within(handle).await.unwrap();
    assert!(outcome.is_err());

    let sidecar = ResumeSidecar::load(&ResumeSidecar::path_for(&destination))
        .expect("expected a sidecar to exist after pausing mid-flight");
    assert!(sidecar.completed.total_bytes() > 0);
    assert!(sidecar.completed.total_bytes() < payload.len() as i64);

    let on_disk = std::fs::read(SparseFile::incomplete_path(&destination)).unwrap();
    for range in sidecar.completed.ranges() {
        let (start, end) = (range.start as usize, range.end as usize);
        assert_eq!(&on_disk[start..end], &payload[start..end]);
    }
}

/// Hazard: `pause()` used to checkpoint unconditionally, including after
/// `start()` had already finished successfully. At that point the file is
/// gone (the `.incomplete` file was already renamed to the destination), but
/// `completed` / `total_bytes` / `validator` still hold their *finished*
/// values, so the unconditional checkpoint would resurrect a `.sdmpart`
/// claiming the whole file is a complete in-progress download — with no
/// `.incomplete` file left to back that claim. A later `start()` against the
/// same destination would trust that sidecar, open a fresh all-zero
/// `.incomplete` file, consider it already complete, and try to finalize
/// garbage over (or in place of) the real file. This is a realistic UI race:
/// the user hits Pause just as the transfer completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_after_successful_completion_does_not_poison_next_start() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    let task = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    let result = within(task.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);

    // The race: pause() lands just after start() already finished.
    task.pause();
    assert!(ResumeSidecar::load(&ResumeSidecar::path_for(&destination)).is_none());

    // A fresh task pointed at the same, already-finished destination has no
    // poisoned sidecar to trust, so it genuinely re-downloads the whole
    // file — and since `destination` still exists, `finalize()` correctly
    // refuses to overwrite it rather than silently replacing (or zero-
    // filling) it. What matters here is that the failure is loud and the
    // original, correct file is left completely untouched.
    let again = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    assert!(within(again.start()).await.is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), payload);
}

/// The deleted-destination variant of the same race: if a stray `pause()`
/// after completion ever did resurrect a sidecar, and the user then deleted
/// the finished file, the next `start()` would have nothing to fail loudly
/// against — `finalize()` would succeed and silently install an all-zero
/// file. Verifying byte identity here closes that path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_after_completion_then_deleting_destination_still_restarts_cleanly() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    let task = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    within(task.start()).await.unwrap();
    task.pause();

    std::fs::remove_file(&destination).unwrap();

    let again = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        4,
        64,
    );
    let result = within(again.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

/// Isolates `prepare()`'s second line of defense from `pause()`'s guard:
/// even if a sidecar on disk claims the whole file is complete (matching
/// total_bytes and validator), if there is no `.incomplete` file backing that
/// claim, `prepare()` must not trust it — it must restart from zero rather
/// than mark the download complete over nonexistent bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sidecar_claiming_complete_without_backing_file_restarts_from_zero() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(4000);

    let mut full = RangeSet::new();
    full.insert(ByteRange::new(0, 4000));
    let poisoned = ResumeSidecar::new(test_source(), None, 4000, None, full);
    poisoned
        .save(&ResumeSidecar::path_for(&destination))
        .unwrap();
    // No `.incomplete` file exists at all — the poisoned case.

    let task = new_task(
        &destination,
        Arc::new(FakeOrigin::new(payload.clone())),
        1,
        64,
    );
    let result: PathBuf = within(task.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}
