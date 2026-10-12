mod support;

use std::sync::Arc;

use sdm_core::{ByteRange, RangeSet};
use sdm_engine::{DownloadError, DownloadTask, FakeOrigin, FakeOriginBehavior};
use support::{
    SeededGenerator, WorkerGate, WorkerGatedOrigin, make_scratch_directory, test_configuration,
    test_payload, test_source, wait_until, within,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

fn new_task(
    destination: std::path::PathBuf,
    transport: Arc<dyn sdm_engine::HttpTransport>,
    workers: usize,
    min_chunk: i64,
) -> Arc<DownloadTask> {
    Arc::new(DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination,
        transport,
        test_configuration(workers, min_chunk),
    ))
}

fn start_in_background(
    task: &Arc<DownloadTask>,
) -> JoinHandle<Result<std::path::PathBuf, DownloadError>> {
    let task = task.clone();
    tokio::spawn(async move { task.start().await })
}

async fn finished(
    handle: JoinHandle<Result<std::path::PathBuf, DownloadError>>,
) -> std::path::PathBuf {
    within(handle).await.unwrap().unwrap()
}

/// Asserts that no two recorded writes overlap. Debug builds only: the write
/// log, like Swift's, exists only under `#[cfg(debug_assertions)]`.
#[cfg(debug_assertions)]
fn assert_writes_disjoint(task: &DownloadTask) {
    let mut seen = RangeSet::new();
    for range in task.write_log() {
        for other in seen.ranges() {
            assert!(
                range.start >= other.end || range.end <= other.start,
                "{range:?} overlaps {other:?}"
            );
        }
        seen.insert(range);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_workers_produce_identical_output() {
    let dir = make_scratch_directory();
    let payload = test_payload(20_000);
    let task = new_task(
        dir.path().join("out.bin"),
        Arc::new(FakeOrigin::new(payload.clone())),
        8,
        64,
    );

    let result = within(task.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

/// A worker's *requested* HTTP range can legitimately overlap another's once
/// claims can be split mid-flight: a claim stolen from a busy worker (see
/// `claim_next`) shrinks that worker's live write boundary without touching
/// the wide `Range` header it already sent — the point of stealing is to
/// avoid re-issuing that request, not to keep requests disjoint. What must
/// stay disjoint is what actually lands on disk, which `write_log`
/// (debug-only) records directly from `record()`, independent of
/// `completed_ranges` — `RangeSet::insert` would silently merge away an
/// overlap rather than reveal one.
#[cfg(debug_assertions)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multiple_workers_never_write_overlapping_bytes() {
    let dir = make_scratch_directory();
    let task = new_task(
        dir.path().join("out.bin"),
        Arc::new(FakeOrigin::new(test_payload(20_000))),
        4,
        64,
    );
    within(task.start()).await.unwrap();

    assert!(task.write_log().len() > 1);
    assert_writes_disjoint(&task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lowering_worker_count_mid_flight_still_completes_file() {
    let dir = make_scratch_directory();
    let payload = test_payload(50_000);
    let behavior = FakeOriginBehavior {
        chunk_size: 256,
        ..Default::default()
    };
    let task = new_task(
        dir.path().join("out.bin"),
        Arc::new(FakeOrigin::with_behavior(payload.clone(), behavior)),
        100,
        64,
    );

    let handle = start_in_background(&task);
    task.set_worker_count(3);
    assert_eq!(std::fs::read(finished(handle).await).unwrap(), payload);
}

/// The Swift test raises the count from a spin on `Task.yield()` against an
/// ungated `FakeOrigin`, which only works because actor hops keep the
/// transfer slow enough. Here the body fetches are held at a `WorkerGate`, so
/// the claims stay in flight until the raise has demonstrably taken effect
/// (the peak rose) and only then does the data flow.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raising_worker_count_mid_flight_still_completes_file() {
    let dir = make_scratch_directory();
    let payload = test_payload(50_000);
    let gate = WorkerGate::new();
    let task = new_task(
        dir.path().join("out.bin"),
        Arc::new(WorkerGatedOrigin::new(payload.clone(), gate.clone())),
        2,
        64,
    );

    let handle = start_in_background(&task);

    // Raise only once both initial workers hold claims, so this exercises a
    // genuine mid-flight raise rather than a target changed before the pool
    // starts.
    wait_until(|| task.active_worker_count() >= 2).await;

    task.set_worker_count(16);
    wait_until(|| task.peak_worker_count() > 2).await;
    gate.open();

    assert_eq!(std::fs::read(finished(handle).await).unwrap(), payload);
    // Byte identity alone would pass even if the raise were a no-op; the peak
    // is what proves extra workers were actually spawned and ran.
    assert!(task.peak_worker_count() > 2);
}

/// Spec §5.1: a fresh claim takes the whole free gap, so a single worker
/// downloading a file owns *all* of it — there is no free remainder left
/// for a raise to hand a second worker the ordinary way. The only source of
/// work for that second worker is stealing the unwritten tail of the first
/// worker's claim. This proves that happens, and — the part that matters
/// most — that the original worker's in-flight request is never restarted:
/// once its claim is stolen from, it keeps streaming the same request it
/// already had open, just stopping early where its shrunk claim ends. Once
/// both workers are down to small remainders they will legitimately keep
/// stealing from each other to finish the file, so this doesn't assert an
/// exact request count — only that the range starting at byte 0 was ever
/// requested once, proving that specific claim was never reissued.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raising_from_one_worker_steals_half_of_the_sole_in_flight_claim_without_restarting_it() {
    let dir = make_scratch_directory();
    let payload = test_payload(50_000);
    let gate = WorkerGate::new();
    let origin = Arc::new(WorkerGatedOrigin::new(payload.clone(), gate.clone()));
    let task = new_task(dir.path().join("out.bin"), origin.clone(), 1, 64);

    let handle = start_in_background(&task);
    wait_until(|| task.active_worker_count() >= 1).await;

    task.set_worker_count(2);
    // The steal has happened once the second worker has run *and* requested
    // the half it took. `peak_worker_count` alone is not enough: it counts the
    // worker from the moment it starts, and if the gate opened before that
    // worker's request went out, the first worker could finish the whole file
    // first and the second would find nothing left to steal.
    wait_until(|| {
        task.peak_worker_count() == 2
            && origin
                .requested_ranges()
                .iter()
                .filter(|range| range.length() > 1)
                .count()
                >= 2
    })
    .await;
    gate.open();

    assert_eq!(std::fs::read(finished(handle).await).unwrap(), payload);
    assert_eq!(task.peak_worker_count(), 2);

    // At least the original claim plus the steal that split it.
    let claims: Vec<ByteRange> = origin
        .requested_ranges()
        .into_iter()
        .filter(|range| range.length() > 1)
        .collect();
    assert!(claims.len() >= 2, "{claims:?}");

    // The byte-0 claim was requested exactly once — a restart would show up
    // as a second, later request also starting at 0.
    assert_eq!(claims.iter().filter(|range| range.start == 0).count(), 1);

    // What was actually written must still be strictly disjoint even though
    // the first worker's *request* spans the whole file.
    #[cfg(debug_assertions)]
    assert_writes_disjoint(&task);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn randomized_worker_churn_preserves_byte_identity() {
    for seed in [1u64, 2, 3, 7, 13, 32] {
        let dir = make_scratch_directory();
        let payload = test_payload(40_000);
        let behavior = FakeOriginBehavior {
            chunk_size: 128,
            ..Default::default()
        };
        let task = new_task(
            dir.path().join("out.bin"),
            Arc::new(FakeOrigin::with_behavior(payload.clone(), behavior)),
            4,
            64,
        );

        let handle = start_in_background(&task);
        let mut generator = SeededGenerator::new(seed);
        for _ in 0..12 {
            task.set_worker_count(generator.in_range(1, 24) as usize);
            tokio::task::yield_now().await;
        }
        let result = finished(handle).await;
        assert_eq!(
            std::fs::read(result).unwrap(),
            payload,
            "byte identity failed for seed {seed}"
        );
    }
}
