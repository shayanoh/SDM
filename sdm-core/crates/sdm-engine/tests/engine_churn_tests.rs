//! The property this port exists to protect, at the engine level: whatever the
//! user does to a library of downloads while they run — changing a download's
//! segment count, stopping and restarting it, changing priorities, reordering,
//! and quitting and relaunching the whole engine — every finished file is
//! byte-identical to its source and nothing but the finished files is left
//! behind. The origin drops connections mid-body throughout.
//!
//! `churn_tests.rs` is the task-level twin (one `DownloadTask`, no engine).
//!
//! Deliberate Rust/Swift differences the test does not "fix": a cancelled
//! worker unwinds at once (so a stopped item may be running again by the time
//! the controller looks), and a mux or wholesale download interrupted by a
//! stop is not a failure (neither is in play here).
//!
//! Reproducing a failure: the seed printed on failure fixes the controller's
//! choices (which action, which item, how many bytes), but when each action
//! lands depends on real worker scheduling under a multi-thread runtime, so
//! the same seed may need several reruns to fail again.
mod support;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, Priority, SourceUrl};
use sdm_engine::{DownloadEngine, FakeOrigin, FakeOriginBehavior, ItemSnapshot, JsonStateStore};
use sha2::{Digest, Sha256};
use support::{
    HostTransport, SeededGenerator, engine_settings, make_scratch_directory, new_engine_with_store,
};
use uuid::Uuid;

const SEEDS: u64 = 20;
const ITEM_COUNT: usize = 6;
/// Fails a seed instead of hanging the suite.
const SEED_TIMEOUT: Duration = Duration::from_secs(120);
/// A backstop on the number of ticks a seed may spend, so a stuck engine fails
/// the test with a message instead of spinning until the timeout.
const MAX_TICKS: usize = 200_000;

/// Which kinds of action landed while a download was actually in flight.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    segment_changes: usize,
    stops: usize,
    starts: usize,
    priority_changes: usize,
    reorders: usize,
    shutdowns: usize,
    /// The most ticks any one seed needed (not summed).
    ticks: usize,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.segment_changes += other.segment_changes;
        self.stops += other.stops;
        self.starts += other.starts;
        self.priority_changes += other.priority_changes;
        self.reorders += other.reorders;
        self.shutdowns += other.shutdowns;
        self.ticks = self.ticks.max(other.ticks);
    }
}

/// A payload distinct per item, so bytes landing in the wrong file are caught.
fn payload_for(index: usize, length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7).wrapping_add(index * 101) % 251) as u8)
        .collect()
}

fn digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

struct Library {
    /// Package name, filename, expected SHA-256, per item.
    expected: Vec<(String, String, Vec<u8>)>,
    transport: HostTransport,
    packages: Vec<DownloadPackage>,
}

fn make_library(rng: &mut SeededGenerator) -> Library {
    let mut origins = Vec::new();
    let mut items = Vec::new();
    let mut expected = Vec::new();
    for index in 0..ITEM_COUNT {
        let length = rng.in_range(2 * 1024 * 1024, 6 * 1024 * 1024) as usize;
        let payload = payload_for(index, length);
        let host = format!("item{index}.example.com");
        let drop_after = rng.in_range(256 * 1024, 1024 * 1024) as usize;
        origins.push((
            host.clone(),
            FakeOrigin::with_behavior(
                payload.clone(),
                FakeOriginBehavior {
                    drop_after_bytes: Some(drop_after),
                    chunk_size: 16 * 1024,
                    ..Default::default()
                },
            ),
        ));
        let filename = format!("file{index}.bin");
        items.push(DownloadItem::http(
            SourceUrl::new(format!("https://{host}/{filename}")).unwrap(),
            filename.clone(),
        ));
        expected.push((
            if index < 3 { "Alpha" } else { "Beta" }.to_string(),
            filename,
            digest(&payload),
        ));
    }
    let beta = items.split_off(3);
    let packages = vec![
        DownloadPackage::new("Alpha").with_items(items),
        DownloadPackage::new("Beta").with_items(beta),
    ];
    Library {
        expected,
        transport: HostTransport::new(origins),
        packages,
    }
}

fn settings(dir: &Path, rng: &mut SeededGenerator) -> sdm_engine::EngineSettings {
    engine_settings(
        rng.in_range(2, 3) as usize,
        rng.in_range(2, 6) as usize,
        16,
        dir,
    )
    // Small, so splits and sidecar checkpoints really happen at these sizes.
    .with_min_segment_size_bytes(64 * 1024)
    .with_checkpoint_interval_bytes(256 * 1024)
    .with_persist_debounce_ticks(2)
}

async fn items_of(engine: &DownloadEngine) -> Vec<ItemSnapshot> {
    engine
        .snapshot()
        .await
        .packages
        .into_iter()
        .flat_map(|package| package.items)
        .collect()
}

fn progress(items: &[ItemSnapshot]) -> i64 {
    items.iter().map(|item| item.completed.total_bytes()).sum()
}

fn running(items: &[ItemSnapshot]) -> Vec<Uuid> {
    items
        .iter()
        .filter(|item| item.state == ItemState::Running)
        .map(|item| item.id)
        .collect()
}

/// Ticks (yielding between ticks, never sleeping) until `reached` holds.
async fn drive(
    engine: &DownloadEngine,
    ticks: &mut usize,
    reached: impl Fn(&[ItemSnapshot]) -> bool,
) -> Result<(), String> {
    loop {
        let items = items_of(engine).await;
        if let Some(failed) = items
            .iter()
            .find(|item| matches!(item.state, ItemState::Failed { .. }))
        {
            return Err(format!(
                "{} failed: {:?} (last failure {:?})",
                failed.filename, failed.state, failed.last_failure_reason
            ));
        }
        if reached(&items) {
            return Ok(());
        }
        if *ticks >= MAX_TICKS {
            return Err(format!("not finished after {MAX_TICKS} ticks: {items:#?}"));
        }
        *ticks += 1;
        engine.tick().await;
        tokio::task::yield_now().await;
    }
}

fn shuffled<T>(rng: &mut SeededGenerator, mut values: Vec<T>) -> Vec<T> {
    for i in (1..values.len()).rev() {
        values.swap(i, rng.in_range(0, i as u64) as usize);
    }
    values
}

fn pick(rng: &mut SeededGenerator, ids: &[Uuid]) -> Option<Uuid> {
    (!ids.is_empty()).then(|| ids[rng.in_range(0, ids.len() as u64 - 1) as usize])
}

fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

async fn run_seed(seed: u64) -> Result<Tally, String> {
    let mut tally = Tally::default();
    let mut rng = SeededGenerator::new(seed);
    let dir = make_scratch_directory();
    let downloads = dir.path().join("downloads");
    let state_path = dir.path().join("state.json");
    let library = make_library(&mut rng);
    let settings = settings(&downloads, &mut rng);

    let mut engine = new_engine_with_store(
        library.transport.clone(),
        Arc::new(JsonStateStore::new(&state_path)),
        settings.clone(),
    );
    for package in &library.packages {
        engine.add(package.clone()).await;
    }

    let steps = rng.in_range(14, 24);
    let shutdown_step = rng.in_range(3, steps - 2);
    let mut shut_down = false;
    let mut stopped: Vec<Uuid> = Vec::new();
    let mut ticks = 0usize;

    for step in 0..steps {
        let items = items_of(&engine).await;
        if items.iter().all(|item| item.state == ItemState::Completed) {
            break;
        }
        // Let some bytes move before the next disruption.
        let target = progress(&items) + rng.in_range(100_000, 1_500_000) as i64;
        drive(&engine, &mut ticks, |items| {
            progress(items) >= target
                // Nothing left that could make progress (the rest is stopped
                // or done): the controller must act, not wait.
                || !items
                    .iter()
                    .any(|item| matches!(item.state, ItemState::Running | ItemState::Queued))
        })
        .await
        .map_err(|message| format!("step {step}: {message}"))?;

        let items = items_of(&engine).await;
        let in_flight = running(&items);
        let all_ids: Vec<Uuid> = items.iter().map(|item| item.id).collect();

        // Stopped items are started again half the time, one at a time.
        if !stopped.is_empty() && rng.in_range(0, 1) == 0 {
            let id = stopped.remove(rng.in_range(0, stopped.len() as u64 - 1) as usize);
            engine.start_item(id).await;
            tally.starts += 1;
        }

        if !shut_down && step >= shutdown_step && !in_flight.is_empty() {
            // Quit and relaunch: a brand new engine over the same store and
            // folder, restoring what the first one left behind.
            engine.shutdown().await;
            engine = new_engine_with_store(
                library.transport.clone(),
                Arc::new(JsonStateStore::new(&state_path)),
                settings.clone(),
            );
            engine.restore().await;
            engine.resume_all().await;
            stopped.clear();
            shut_down = true;
            tally.shutdowns += 1;
            continue;
        }

        match rng.in_range(0, 9) {
            0..=2 => {
                let count = rng.in_range(1, 8) as usize;
                if let Some(id) = pick(&mut rng, &in_flight) {
                    engine.set_segment_count(count, id).await;
                    tally.segment_changes += 1;
                } else if let Some(id) = pick(&mut rng, &all_ids) {
                    engine.set_segment_count(count, id).await;
                }
            }
            3 | 4 => {
                if let Some(id) = pick(&mut rng, &in_flight) {
                    engine.stop_item(id).await;
                    stopped.push(id);
                    tally.stops += 1;
                }
            }
            5 | 6 => {
                let priorities = [
                    Priority::Lowest,
                    Priority::Low,
                    Priority::Normal,
                    Priority::High,
                    Priority::Highest,
                ];
                let priority =
                    (rng.in_range(0, 5) < 5).then(|| priorities[rng.in_range(0, 4) as usize]);
                if let Some(id) = pick(&mut rng, &all_ids) {
                    engine.set_priority(priority, id).await;
                    if !in_flight.is_empty() {
                        tally.priority_changes += 1;
                    }
                }
            }
            7 | 8 => {
                let snapshot = engine.snapshot().await;
                let package = &snapshot.packages[rng.in_range(0, 1) as usize];
                let ids: Vec<Uuid> = package.items.iter().map(|item| item.id).collect();
                engine
                    .reorder_items(&shuffled(&mut rng, ids), package.id)
                    .await;
                if !in_flight.is_empty() {
                    tally.reorders += 1;
                }
            }
            _ => {}
        }
    }

    // Everything the controller stopped is started again, then the engine
    // runs to completion on ticks alone.
    engine.resume_all().await;
    drive(&engine, &mut ticks, |items| {
        items.iter().all(|item| item.state == ItemState::Completed)
    })
    .await
    .map_err(|message| format!("final drive: {message}"))?;
    engine.shutdown().await;

    // Every output file is its payload.
    for (package, filename, expected) in &library.expected {
        let path = downloads.join(package).join(filename);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("{} unreadable: {error}", path.display()))?;
        if digest(&bytes) != *expected {
            return Err(format!("{} does not hash to its payload", path.display()));
        }
    }
    // And nothing but the finished files is left behind.
    let mut leftovers = Vec::new();
    files_under(&downloads, &mut leftovers);
    let incomplete_or_sidecar: Vec<&PathBuf> = leftovers
        .iter()
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.ends_with(".incomplete") || name.ends_with(".sdmpart")
        })
        .collect();
    if !incomplete_or_sidecar.is_empty() {
        return Err(format!("left behind: {incomplete_or_sidecar:?}"));
    }
    if leftovers.len() != ITEM_COUNT {
        return Err(format!(
            "expected exactly the {ITEM_COUNT} finished files, found {leftovers:?}"
        ));
    }
    tally.ticks = ticks;
    Ok(tally)
}

async fn within_seed_timeout<T>(seed: u64, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(SEED_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("engine churn seed {seed}: timed out, the engine hung"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_byte_identity_under_churn() {
    let mut total = Tally::default();
    for seed in 1..=SEEDS {
        let outcome = within_seed_timeout(seed, run_seed(seed)).await;
        match outcome {
            Ok(tally) => total.add(tally),
            Err(message) => panic!("engine churn seed {seed}: {message}"),
        }
    }
    println!("engine churn over {SEEDS} seeds: {total:?}");
    // The property is only worth something if the disruptions happened while
    // downloads were in flight.
    assert!(
        total.segment_changes > 0,
        "no segment change landed: {total:?}"
    );
    assert!(total.stops > 0, "no stop landed: {total:?}");
    assert!(total.starts > 0, "no start landed: {total:?}");
    assert!(
        total.priority_changes > 0,
        "no priority change landed: {total:?}"
    );
    assert!(total.reorders > 0, "no reorder landed: {total:?}");
    assert!(total.shutdowns > 0, "no shutdown landed: {total:?}");
}
