//! Port of Swift's `DownloadEngineTests.swift` (15 tests).
mod support;

use std::sync::Arc;
use std::time::Duration;

use sdm_core::{AppTiming, ByteRange, DownloadPackage, ItemState, Priority, RangeSet};
use sdm_engine::{
    DownloadEngine, FakeOrigin, InMemoryStateStore, PersistedState, ResumeSidecar, RetryPolicy,
    StateStore,
};
use support::{
    Gate, GatePhase, engine_settings, example_item, make_gated_engine, make_scratch_directory,
    new_engine, package_of, snapshot_item, test_payload, wait_for_progress, within,
};

fn make_engine(
    payload: Vec<u8>,
    folder: &std::path::Path,
    max_concurrent: usize,
    segments: usize,
) -> DownloadEngine {
    new_engine(
        FakeOrigin::new(payload),
        engine_settings(max_concurrent, segments, 32, folder),
    )
}

fn urgent_package(name: &str) -> DownloadPackage {
    DownloadPackage::new("Urgent")
        .with_items(vec![example_item(name).with_position(0)])
        .with_priority(Priority::Highest)
        .with_position(1)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_downloads_all_items_into_a_package_folder() {
    let dir = make_scratch_directory();
    let payload = test_payload(3000);

    let engine = make_engine(payload.clone(), dir.path(), 2, 2);
    engine.add(package_of(&["a.bin", "b.bin"])).await;
    within(engine.run_until_idle()).await.unwrap();

    let package_folder = dir.path().join("Batch");
    assert_eq!(
        std::fs::read(package_folder.join("a.bin")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::read(package_folder.join("b.bin")).unwrap(),
        payload
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabled_items_are_not_downloaded() {
    let dir = make_scratch_directory();

    let engine = make_engine(test_payload(1000), dir.path(), 2, 2);
    let mut package = package_of(&["a.bin", "b.bin"]);
    package.items[1].is_enabled = false;
    engine.add(package).await;
    within(engine.run_until_idle()).await.unwrap();

    let folder = dir.path().join("Batch");
    assert!(folder.join("a.bin").exists());
    assert!(!folder.join("b.bin").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_limit_is_respected() {
    let dir = make_scratch_directory();

    let engine = make_engine(test_payload(20_000), dir.path(), 2, 2);
    engine
        .add(package_of(&["a.bin", "b.bin", "c.bin", "d.bin"]))
        .await;
    engine.tick().await;

    let running = engine
        .snapshot()
        .await
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .filter(|item| item.state == ItemState::Running)
        .count();
    assert!(running <= 2, "{running} running");

    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_aggregates_speed_across_items() {
    let dir = make_scratch_directory();

    let engine = make_engine(test_payload(4000), dir.path(), 2, 2);
    engine.add(package_of(&["a.bin", "b.bin"])).await;
    within(engine.run_until_idle()).await.unwrap();
    engine.tick().await;

    let snapshot = engine.snapshot().await;
    let item_total: f64 = snapshot
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .map(|item| item.bytes_per_second)
        .sum();
    assert!((snapshot.global_bytes_per_second - item_total).abs() < 0.001);
    // The aggregate must be a real number, not two zeroes agreeing with
    // each other: 8000 bytes across two items landed in one window.
    assert!(snapshot.global_bytes_per_second > 0.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reports_segmented_progress_ranges() {
    let dir = make_scratch_directory();

    let engine = make_engine(test_payload(2000), dir.path(), 2, 2);
    engine.add(package_of(&["a.bin"])).await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = engine.snapshot().await;
    let item = &snapshot.packages[0].items[0];
    assert_eq!(item.completed.ranges(), [ByteRange::new(0, 2000)]);
    assert_eq!(item.state, ItemState::Completed);
    assert_eq!(item.total_bytes, Some(2000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_flushes_state_to_the_store() {
    let dir = make_scratch_directory();

    let store = Arc::new(InMemoryStateStore::new());
    let engine = DownloadEngine::new(
        Arc::new(FakeOrigin::new(test_payload(1000))),
        store.clone(),
        engine_settings(1, 1, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        None,
    );
    engine.add(package_of(&["a.bin"])).await;
    engine.shutdown().await;

    let names: Vec<_> = store
        .load()
        .await
        .packages
        .into_iter()
        .map(|package| package.name)
        .collect();
    assert_eq!(names, ["Batch"]);
}

/// Preemption unwinds a runner by cancelling its job and pausing its task, so
/// `start()` fails — exactly the shape a real failure has. Charging it to the
/// retry budget would let a perfectly healthy download be marked `Failed` for
/// having been outranked too often, and would hold it in backoff instead of
/// letting it resume the moment a slot frees up.
///
/// `max_attempts: 1` makes any charge at all immediately terminal, so the
/// assertion cannot pass by accident.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preemption_is_not_charged_to_the_retry_budget() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);

    let gate = Gate::new();
    let engine = make_gated_engine(
        payload.clone(),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        RetryPolicy::new()
            .with_max_attempts(1)
            .with_base_delay(Duration::from_secs(30)),
    );

    let package = package_of(&["a.bin"]);
    let item_id = package.items[0].id;
    engine.add(package).await;

    gate.wait_for_arrival().await;
    // Disabling retires the in-flight runner through the preemption path.
    engine.set_enabled(false, item_id).await;
    gate.open();
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(item_id, &engine).await.unwrap().state,
        ItemState::Stopped
    );

    // A 30 s backoff would keep it out of the desired set for 30 ticks; a
    // charged attempt against max_attempts 1 would have made it terminal. It
    // must simply start again on the very next reconcile.
    engine.set_enabled(true, item_id).await;
    engine.start_item(item_id).await;
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(item_id, &engine).await.unwrap().state,
        ItemState::Completed
    );
    assert_eq!(
        std::fs::read(dir.path().join("Batch").join("a.bin")).unwrap(),
        payload
    );
}

/// Spec §4.3 requires the sidecar be checkpointed "every ~8 MB per worker or
/// every 5 s, whichever comes first". `DownloadTask::checkpoint_tick()` is the
/// wall-clock half; if the engine's `tick()` never calls it, the trigger is
/// dead code in production and the spec is unmet. This parks a download
/// mid-body — far below the 8 MB byte trigger — and shows that ticking alone
/// produces the sidecar, on the tick the staleness threshold is reached and
/// not before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ticking_checkpoints_running_downloads() {
    let dir = make_scratch_directory();

    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(4000),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        RetryPolicy::new(),
    );
    let package = package_of(&["a.bin"]);
    let item_id = package.items[0].id;
    engine.add(package).await;

    // `wait_for_arrival` only proves the *producer* reached the gate. Wait
    // until the worker has actually consumed and recorded the first slice, so
    // the sidecar assertions below cannot depend on scheduler luck.
    gate.wait_for_arrival().await;
    wait_for_progress(item_id, &engine).await;

    let destination = dir.path().join("Batch").join("a.bin");
    let sidecar_path = ResumeSidecar::path_for(&destination);

    for _ in 0..(AppTiming::TICKS_PER_SECOND * 5 - 1) {
        engine.tick().await;
    }
    assert!(!sidecar_path.exists());

    engine.tick().await;
    assert!(sidecar_path.exists());
    let sidecar = ResumeSidecar::load(&sidecar_path).expect("a readable sidecar");
    assert!(sidecar.completed.total_bytes() > 0);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), test_payload(4000));
}

/// `DownloadTask::start()` awaits `transport.fetch` inside `prepare()`. A
/// `pause()` arriving at that suspension point used to be silently discarded:
/// it set the target worker count to 0, but `run_workers()` then reset it to
/// the configured count and the download proceeded anyway. The engine preempts
/// by pausing, so it reaches this window for real. Disabling an item is the
/// deterministic way to drive it — the item is then ineligible, so nothing
/// restarts it and the assertions cannot be masked by a later attempt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_during_prepare_actually_stops_the_download() {
    let dir = make_scratch_directory();

    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(4000),
        dir.path(),
        &gate,
        GatePhase::Probe,
        false,
        RetryPolicy::new(),
    );
    let package = package_of(&["a.bin"]);
    let item_id = package.items[0].id;
    engine.add(package).await;

    // prepare() is now suspended inside transport.fetch.
    gate.wait_for_arrival().await;
    engine.set_enabled(false, item_id).await;
    gate.open();
    within(engine.run_until_idle()).await.unwrap();

    let item = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(item.state, ItemState::Stopped);
    assert_eq!(item.completed.total_bytes(), 0);
    assert!(!dir.path().join("Batch").join("a.bin").exists());
}

/// Preemption in production shape: nothing here hand-sets `is_resumable`. The
/// items are built exactly as a grabber would hand them over — flag `None`,
/// meaning "not probed yet" — and it is the engine that learns the origin
/// honors `Range` and writes `true` onto the item, which is what lets spec §6
/// pass 1 fall through and the higher-priority `b.bin` take the only slot.
///
/// A preempted item must come back as `Queued` and keep the bytes it already
/// had, so the later attempt resumes rather than restarts — proved by the
/// byte-identity assertion after the drain.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preempted_item_returns_to_queued_and_resumes_later() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);

    let gate = Gate::new();
    let engine = make_gated_engine(
        payload.clone(),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        RetryPolicy::new(),
    );

    let first = package_of(&["a.bin"]);
    let item_id = first.items[0].id;
    assert_eq!(first.items[0].is_resumable(), None);
    engine.add(first).await;

    // Wait for genuine mid-flight progress — a reserved claim alone would let
    // the progress-preservation assertion below pass trivially.
    gate.wait_for_arrival().await;
    wait_for_progress(item_id, &engine).await;

    // Past the hysteresis window (spec §6.4), so the urgent package below is
    // free to preempt it — this test is about preservation of progress
    // across a preemption, not about hysteresis itself (see hysteresis_tests).
    for _ in 0..(AppTiming::TICKS_PER_SECOND * 5) {
        engine.tick().await;
    }

    engine.add(urgent_package("b.bin")).await;

    let midway = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(midway.state, ItemState::Queued);
    assert!(midway.completed.total_bytes() > 0);
    // The engine, not the test, discovered this.
    assert_eq!(midway.is_resumable, Some(true));

    gate.open();
    within(engine.run_until_idle()).await.unwrap();

    let batch = dir.path().join("Batch");
    let urgent = dir.path().join("Urgent");
    assert_eq!(std::fs::read(batch.join("a.bin")).unwrap(), payload);
    assert_eq!(std::fs::read(urgent.join("b.bin")).unwrap(), payload);

    // Telemetry must account for the payload exactly once across the whole
    // preempt/resume cycle. Seeding the sampler baseline from the item's
    // resumed progress is what prevents the pre-preemption span being counted
    // a second time by the second attempt; seeding it at zero would report
    // 6000 bytes moved for a 4000-byte file.
    //
    // Each history entry is a bytes/second *rate estimate*
    // (`pending_bytes * TICKS_PER_SECOND`, since a tick only covers
    // `1 / TICKS_PER_SECOND` of a second), not a raw per-tick byte count —
    // summing every window's rate sample therefore recovers the payload
    // size only after scaling back down by `TICKS_PER_SECOND`.
    engine.tick().await;
    let finished = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(
        finished.speed_history.iter().sum::<f64>(),
        payload.len() as f64 * AppTiming::TICKS_PER_SECOND as f64
    );
}

/// The other half of the ruling, and the transition that actually matters:
/// `None -> Some(false)`. Against an origin that ignores `Range`, the download
/// cannot be resumed, so preempting it would throw away every byte it has. The
/// engine must learn that from the probe and write it onto the item, at which
/// point spec §6.3 pass 1 gives it an unconditional claim on its slot and the
/// higher-priority `b.bin` has to wait — the exact opposite outcome to
/// `preempted_item_returns_to_queued_and_resumes_later`, from the same setup
/// and with the flag again never touched by the test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_resumable_running_item_is_not_preempted_once_probed() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);

    let gate = Gate::new();
    let engine = make_gated_engine(
        payload.clone(),
        dir.path(),
        &gate,
        GatePhase::Body,
        true,
        RetryPolicy::new(),
    );

    let first = package_of(&["a.bin"]);
    let item_id = first.items[0].id;
    assert_eq!(first.items[0].is_resumable(), None);
    engine.add(first).await;

    gate.wait_for_arrival().await;
    wait_for_progress(item_id, &engine).await;

    let second = urgent_package("b.bin");
    let urgent_id = second.items[0].id;
    engine.add(second).await;

    let held = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(held.is_resumable, Some(false));
    assert_eq!(held.state, ItemState::Running);
    let waiting = snapshot_item(urgent_id, &engine).await.unwrap();
    assert_eq!(waiting.state, ItemState::Queued);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        std::fs::read(dir.path().join("Batch").join("a.bin")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::read(dir.path().join("Urgent").join("b.bin")).unwrap(),
        payload
    );
}

/// Isolates the scheduler rule itself. `refresh_resumability()` normally lands
/// before any scheduling decision, so both `is_resumable == Some(false)` and
/// the naive `!= Some(true)` produce the same answer once a probe has
/// returned. They differ only in the window this test occupies: a runner
/// exists but its probe has not answered, so the flag is genuinely `None`.
/// Treating unknown as non-resumable would hand that item an unconditional
/// claim on the only slot and make it unpreemptible — for a download that has
/// not transferred a single byte and has nothing to lose.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unprobed_running_item_is_still_preemptible() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);

    let gate = Gate::new();
    let engine = make_gated_engine(
        payload.clone(),
        dir.path(),
        &gate,
        GatePhase::Probe,
        false,
        RetryPolicy::new(),
    );

    let first = package_of(&["a.bin"]);
    let item_id = first.items[0].id;
    engine.add(first).await;

    // Parked inside prepare(); the probe has not answered, so nothing knows
    // yet whether this download is resumable.
    gate.wait_for_arrival().await;

    // Past the hysteresis window (spec §6.4) so the urgent package below is
    // free to preempt it — this test is about preemptibility while unprobed,
    // not about hysteresis itself (see hysteresis_tests).
    for _ in 0..(AppTiming::TICKS_PER_SECOND * 5) {
        engine.tick().await;
    }

    let second = urgent_package("b.bin");
    let urgent_id = second.items[0].id;
    engine.add(second).await;

    let unprobed = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(unprobed.is_resumable, None);
    assert_eq!(unprobed.state, ItemState::Queued);
    let urgent = snapshot_item(urgent_id, &engine).await.unwrap();
    assert_eq!(urgent.state, ItemState::Running);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        std::fs::read(dir.path().join("Batch").join("a.bin")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::read(dir.path().join("Urgent").join("b.bin")).unwrap(),
        payload
    );
}

// MARK: - Restore

/// Engine-level mirror of the `DownloadTask` resume tests: a package graph
/// with partial progress and a `Running` item is placed directly in the
/// store — the shape a hard crash (no graceful `shutdown()`) would leave
/// behind, since `shutdown()` itself pauses every runner before persisting
/// and so never writes `Running` back out. A fresh engine over that same
/// store must bring the package back with its progress intact and land the
/// previously-`Running` item somewhere sane, not replay it as still running.
///
/// The item starts disabled so `restore()`'s internal `reconcile()` does not
/// immediately spin up a runner and overwrite the very snapshot fields this
/// test is asserting on — `set_enabled(true, ...)` flips it live afterward, to
/// prove the restored progress is real (the previously "completed" 1000
/// bytes are not backed by any on-disk `.sdmpart`/`.incomplete`, per
/// `DownloadTask::prepare()`'s validator guard, so the resumed run
/// legitimately restarts that item from zero and still has to reach byte
/// identity).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restore_repopulates_packages_with_progress_intact() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);

    let mut package = package_of(&["a.bin", "b.bin"]);
    let item_id = package.items[0].id;
    package.items[0].state = ItemState::Running;
    package.items[0].is_enabled = false;
    package.items[0].set_completed(RangeSet::from_ranges(&[ByteRange::new(0, 1000)]));
    package.items[0].set_total_bytes(Some(4000));
    package.items[0].set_is_resumable(Some(true));

    let store = Arc::new(InMemoryStateStore::new());
    store.save(PersistedState::new(vec![package])).await;

    let engine = DownloadEngine::new(
        Arc::new(FakeOrigin::new(payload.clone())),
        store,
        engine_settings(1, 2, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        None,
    );
    engine.restore().await;

    let restored_packages = engine.snapshot().await.packages;
    let names: Vec<_> = restored_packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Batch"]);
    let restored_item = restored_packages[0]
        .items
        .iter()
        .find(|item| item.id == item_id)
        .expect("the restored item");
    // Landing state: never `Running`/`Queued` on arrival — nothing in this
    // process is actually running it or about to, and reporting otherwise
    // would misinform both the scheduler and the UI. `persist()` never writes
    // `Running`/`Queued` either (see its doc comment), so every restored
    // non-terminal item lands `Stopped` regardless of what it was doing when
    // the previous process quit.
    assert_eq!(restored_item.state, ItemState::Stopped);
    // Progress from the store survives the round trip verbatim (no runner
    // exists for a disabled item, so this reads straight from the restored
    // item rather than a live task).
    assert_eq!(restored_item.completed.total_bytes(), 1000);
    // is_resumable resets for re-probing rather than trusting the previous
    // process's answer, which could otherwise make the item unpreemptible
    // before this process has verified anything about the origin.
    assert_eq!(restored_item.is_resumable, None);

    // Re-enabling alone does not resume a stopped item — same as everywhere
    // else, the user still has to start it explicitly. "b.bin" was never
    // disabled, so `resume_all()` alone (the app layer's "resume on launch")
    // is enough for it; "a.bin" also needs re-enabling first.
    engine.set_enabled(true, item_id).await;
    engine.resume_all().await;
    within(engine.run_until_idle()).await.unwrap();
    let batch = dir.path().join("Batch");
    assert_eq!(std::fs::read(batch.join("a.bin")).unwrap(), payload);
    assert_eq!(std::fs::read(batch.join("b.bin")).unwrap(), payload);
}

/// `restore()` is idempotent (a second call does not duplicate the restored
/// package) and composes with `add()` called afterward, which is the order
/// `EngineController` actually uses — `restore()` runs once at the head of
/// `startHeartbeat()`, before any user-driven `add()` can occur.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restore_is_idempotent_and_composes_with_subsequent_adds() {
    let dir = make_scratch_directory();

    let store = Arc::new(InMemoryStateStore::new());
    store
        .save(PersistedState::new(vec![package_of(&["stored.bin"])]))
        .await;

    let engine = DownloadEngine::new(
        Arc::new(FakeOrigin::new(test_payload(10))),
        store,
        engine_settings(1, 1, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        None,
    );

    engine.restore().await;
    engine.restore().await; // must not duplicate the first call's work

    let filenames = |snapshot: sdm_engine::EngineSnapshot| -> Vec<String> {
        snapshot
            .packages
            .into_iter()
            .flat_map(|package| package.items)
            .map(|item| item.filename)
            .collect()
    };
    assert_eq!(filenames(engine.snapshot().await), ["stored.bin"]);

    engine.add(package_of(&["live.bin"])).await;
    let mut names = filenames(engine.snapshot().await);
    names.sort();
    assert_eq!(names, ["live.bin", "stored.bin"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_item_carries_its_source_url() {
    let dir = make_scratch_directory();
    let engine = make_engine(test_payload(100), dir.path(), 1, 2);

    let item = example_item("a.bin");
    let url = item.source_url.clone();
    engine
        .add(DownloadPackage::new("Pkg").with_items(vec![item]))
        .await;

    let snapshot = engine.snapshot().await;
    assert_eq!(snapshot.packages[0].items[0].url, url);
}
