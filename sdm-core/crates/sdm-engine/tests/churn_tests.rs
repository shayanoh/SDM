//! The property this port exists to protect: whatever happens to a download
//! while it runs — the worker count changing, a pause and a restart from the
//! sidecar, the task being dropped outright, an origin that drops or
//! truncates bodies — the finished file is byte-identical to the source, and
//! at every stop the sidecar describes only bytes that really are on disk.
mod support;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use sdm_core::{ByteRange, RangeSet};
use sdm_engine::{
    DownloadError, DownloadTask, DownloadTaskConfiguration, FakeOrigin, FakeOriginBehavior,
    ResumeSidecar, SparseFile, TransportError,
};
use sha2::{Digest, Sha256};
use support::{SeededGenerator, make_scratch_directory, test_payload, test_source};
use uuid::Uuid;

const SEEDS: u64 = 40;
const PAYLOAD_LEN: usize = 8 * 1024 * 1024;
const MAX_ATTEMPTS: u64 = 300;
/// Fails a seed instead of hanging the suite.
const SEED_TIMEOUT: Duration = Duration::from_secs(120);

enum Action {
    SetWorkers(usize),
    Pause,
    Kill,
}

enum AttemptEnd {
    Finished(PathBuf),
    Failed(DownloadError),
    Paused,
    Killed,
}

fn clean_behavior() -> FakeOriginBehavior {
    FakeOriginBehavior {
        chunk_size: 4096,
        ..Default::default()
    }
}

/// Clean, dropping mid-body, or ending the body early without an error.
fn random_behavior(rng: &mut SeededGenerator) -> FakeOriginBehavior {
    let limit = rng.in_range(8 * 1024, 1024 * 1024) as usize;
    match rng.in_range(0, 2) {
        0 => FakeOriginBehavior {
            drop_after_bytes: Some(limit),
            ..clean_behavior()
        },
        1 => FakeOriginBehavior {
            truncate_after_bytes: Some(limit),
            ..clean_behavior()
        },
        _ => clean_behavior(),
    }
}

fn random_action(rng: &mut SeededGenerator, allow_disruption: bool) -> Action {
    let roll = if allow_disruption {
        rng.in_range(0, 9)
    } else {
        0
    };
    match roll {
        0..=5 => Action::SetWorkers(rng.in_range(1, 16) as usize),
        6 | 7 => Action::Pause,
        _ => Action::Kill,
    }
}

async fn wait_for_progress(task: &DownloadTask, target: i64) {
    while task.completed_ranges().total_bytes() < target {
        tokio::task::yield_now().await;
    }
}

fn end_of(result: Result<PathBuf, DownloadError>) -> AttemptEnd {
    match result {
        Ok(path) => AttemptEnd::Finished(path),
        Err(error) => AttemptEnd::Failed(error),
    }
}

/// Runs one `DownloadTask`: `steps` times, waits for a seeded amount of
/// progress and then applies a seeded action, and finally lets the run end.
async fn run_attempt(
    task: &Arc<DownloadTask>,
    rng: &mut SeededGenerator,
    steps: u64,
    allow_disruption: bool,
) -> AttemptEnd {
    let mut start = Some(Box::pin(task.start()));
    for _ in 0..steps {
        let target = task.completed_ranges().total_bytes() + rng.in_range(1, 1_500_000) as i64;
        let action = random_action(rng, allow_disruption);
        let running = start.as_mut().expect("the run is still in hand");
        tokio::select! {
            result = &mut *running => return end_of(result),
            _ = wait_for_progress(task, target) => {}
        }
        match action {
            Action::SetWorkers(count) => task.set_worker_count(count),
            Action::Pause => {
                task.pause();
                let result = start.take().expect("the run is still in hand").await;
                return match result {
                    Ok(path) => AttemptEnd::Finished(path),
                    Err(_) => AttemptEnd::Paused,
                };
            }
            // Dropping the future is the "kill": no code of the test runs
            // inside the download, exactly as if its caller walked away.
            Action::Kill => {
                drop(start.take());
                return AttemptEnd::Killed;
            }
        }
    }
    end_of(start.take().expect("the run is still in hand").await)
}

/// What the sidecar vouches for must be on disk, byte for byte.
fn check_sidecar(destination: &Path, payload: &[u8]) -> Result<(), String> {
    let Some(sidecar) = ResumeSidecar::load(&ResumeSidecar::path_for(destination)) else {
        return Ok(());
    };
    let on_disk = std::fs::read(SparseFile::incomplete_path(destination))
        .map_err(|error| format!("a sidecar without a readable .incomplete file: {error}"))?;
    if on_disk.len() != payload.len() {
        return Err(format!(
            ".incomplete is {} bytes, expected {}",
            on_disk.len(),
            payload.len()
        ));
    }
    for set in [Some(&sidecar.completed), sidecar.durable_completed.as_ref()]
        .into_iter()
        .flatten()
    {
        for range in set.ranges() {
            let (start, end) = (range.start as usize, range.end as usize);
            if end > payload.len() || on_disk[start..end] != payload[start..end] {
                return Err(format!(
                    "the sidecar records {range:?} but the bytes on disk differ"
                ));
            }
        }
    }
    Ok(())
}

/// How the attempts of a seed ended, so the test can show it exercised every
/// kind of disruption rather than only passing.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    attempts: usize,
    failed: usize,
    paused: usize,
    killed: usize,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.attempts += other.attempts;
        self.failed += other.failed;
        self.paused += other.paused;
        self.killed += other.killed;
    }
}

async fn run_seed(seed: u64, payload: Bytes, expected: Sha256) -> Result<Tally, String> {
    let mut tally = Tally::default();
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let origin = FakeOrigin::with_behavior(payload.clone(), clean_behavior());
    let mut rng = SeededGenerator::new(seed);
    // After this many attempts the origin turns well-behaved, so the
    // download is certain to end.
    let chaotic_attempts = rng.in_range(6, 14);

    for attempt in 0..MAX_ATTEMPTS {
        let chaotic = attempt < chaotic_attempts;
        origin.set_behavior(if chaotic {
            random_behavior(&mut rng)
        } else {
            clean_behavior()
        });
        let task = Arc::new(DownloadTask::new(
            Uuid::new_v4(),
            test_source(),
            destination.clone(),
            Arc::new(origin.clone()),
            DownloadTaskConfiguration::new(
                rng.in_range(1, 16) as usize,
                16 * 1024,
                256 * 1024,
                None,
            ),
        ));
        let steps = rng.in_range(if chaotic { 1 } else { 0 }, 5);
        tally.attempts += 1;
        match run_attempt(&task, &mut rng, steps, chaotic).await {
            AttemptEnd::Finished(path) => {
                let finished = std::fs::read(&path).map_err(|error| error.to_string())?;
                if Sha256::new().chain_update(&finished).finalize() != expected.clone().finalize() {
                    return Err("the finished file's SHA-256 differs from the source".into());
                }
                let whole = vec![ByteRange::new(0, payload.len() as i64)];
                if task.completed_ranges() != RangeSet::from_ranges(&whole) {
                    return Err(format!(
                        "the final range set is {:?}, not [0, {})",
                        task.completed_ranges(),
                        payload.len()
                    ));
                }
                return Ok(tally);
            }
            AttemptEnd::Failed(error) => {
                tally.failed += 1;
                // Only the failures this origin can cause.
                let expected_failure = matches!(
                    error,
                    DownloadError::Transport(TransportError::ConnectionDropped)
                        | DownloadError::TruncatedResponse { .. }
                        | DownloadError::IncompleteAfterWorkersFinished
                );
                if !expected_failure {
                    return Err(format!("attempt {attempt} failed unexpectedly: {error:?}"));
                }
            }
            AttemptEnd::Paused => tally.paused += 1,
            AttemptEnd::Killed => tally.killed += 1,
        }
        check_sidecar(&destination, &payload)
            .map_err(|message| format!("after attempt {attempt}: {message}"))?;
    }
    Err(format!("not finished after {MAX_ATTEMPTS} attempts"))
}

async fn within_seed_timeout<T>(seed: u64, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(SEED_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("churn seed {seed}: timed out, the download hung"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn byte_identity_under_randomized_churn() {
    let payload = Bytes::from(test_payload(PAYLOAD_LEN));
    let expected = Sha256::new().chain_update(&payload);
    let mut total = Tally::default();
    for seed in 1..=SEEDS {
        match within_seed_timeout(seed, run_seed(seed, payload.clone(), expected.clone())).await {
            Ok(tally) => total.add(tally),
            Err(message) => panic!("churn seed {seed}: {message}"),
        }
    }
    println!("churn over {SEEDS} seeds: {total:?}");
    // The property is only worth something if the disruptions happened.
    assert!(total.failed > 0, "no attempt ever failed: {total:?}");
    assert!(total.paused > 0, "no attempt was ever paused: {total:?}");
    assert!(total.killed > 0, "no attempt was ever killed: {total:?}");
}
