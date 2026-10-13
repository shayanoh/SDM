//! Port of Swift's `RemovalTests.swift` (12 tests), plus four of our own at the end.
//!
//! Swift's removal tests put files into the user's real Trash through
//! `FileManager.trashItem`. These run against `ScratchTrash`, which moves them
//! into a scratch directory instead, and a few also check what it was handed.
mod support;

use std::sync::Arc;

use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use support::{
    Gate, GatePhase, ScratchTrash, WorkerGate, WorkerGatedOrigin, engine_settings, example_item,
    make_gated_engine, make_scratch_directory, new_engine, new_engine_with_trash, running_count,
    snapshot_item, test_payload, test_source, within,
};
use uuid::Uuid;

fn a_bin() -> DownloadItem {
    DownloadItem::http(test_source(), "a.bin")
}

/// An item the user just resumed is no longer stopped: it is `Queued`, or
/// already `Running` again.
///
/// Swift's tests assert `.queued` exactly, which holds there because a Swift
/// worker parked at the test gate cannot unwind until the gate opens, so the
/// old runner still occupies the item. Here a cancelled worker drops its
/// parked fetch and unwinds at once, so whether the old runner is gone by the
/// time the engine reschedules is a race the engine is free to win.
fn assert_resumed(state: &ItemState) {
    assert!(
        matches!(state, ItemState::Queued | ItemState::Running),
        "expected a resumed item to be queued or running, found {state:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_item_without_deleting_file_keeps_bytes_on_disk() {
    let dir = make_scratch_directory();
    let trash = Arc::new(ScratchTrash::new());
    let engine = new_engine_with_trash(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
        trash.clone(),
    );
    let item = a_bin();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let destination = dir.path().join("Batch").join("a.bin");
    assert!(destination.exists());

    engine.remove_item(item.id, false).await;

    assert!(destination.exists());
    assert!(engine.snapshot().await.packages.is_empty());
    assert!(trash.trashed().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_item_deleting_file_trashes_it_and_drops_the_empty_package() {
    let dir = make_scratch_directory();
    let trash = Arc::new(ScratchTrash::new());
    let engine = new_engine_with_trash(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
        trash.clone(),
    );
    let item = a_bin();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let destination = dir.path().join("Batch").join("a.bin");
    assert!(destination.exists());

    engine.remove_item(item.id, true).await;

    assert!(!destination.exists());
    assert!(engine.snapshot().await.packages.is_empty());
    // The file went to the trash, and so did the folder it left empty.
    assert_eq!(trash.trashed(), [destination, dir.path().join("Batch")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_package_deleting_files_trashes_the_whole_folder() {
    let dir = make_scratch_directory();
    let trash = Arc::new(ScratchTrash::new());
    let engine = new_engine_with_trash(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
        trash.clone(),
    );
    let items: Vec<DownloadItem> = (0..2)
        .map(|index| example_item(&format!("{index}.bin")))
        .collect();
    let package_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("Batch")
                .with_id(package_id)
                .with_items(items),
        )
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let folder = dir.path().join("Batch");
    assert!(folder.exists());

    engine.remove_package(package_id, true).await;

    assert!(!folder.exists());
    assert!(engine.snapshot().await.packages.is_empty());
    assert_eq!(trash.trashed(), [folder]);
}

/// A reset must not itself start a download — it only discards progress.
/// Resetting a `Completed` item has neither a "stays queued" nor a "stays
/// stopped" prior state to preserve, so it lands `Stopped` like a `Failed` one
/// would: the bytes are gone and nothing should pick it back up until the
/// operator explicitly says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_download_discards_bytes_and_does_not_auto_start() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = a_bin();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(item.id, &engine).await.unwrap().state,
        ItemState::Completed
    );

    engine.reset_download(item.id).await;

    let reset = snapshot_item(item.id, &engine).await.unwrap();
    assert_eq!(reset.state, ItemState::Stopped);
    assert_eq!(reset.completed.total_bytes(), 0);
    assert_eq!(reset.total_bytes, None);

    let destination = dir.path().join("Batch").join("a.bin");
    assert!(!destination.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_download_preserves_a_queued_items_scheduling_state() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    // A single concurrency slot, held indefinitely by `running` — `target`
    // can never be promoted off `Queued` for the whole test, regardless of
    // scheduling order, which is what makes this deterministic.
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(10), gate.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );
    let running = example_item("running.bin");
    let target = a_bin();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![running, target.clone()]))
        .await;
    assert_eq!(
        snapshot_item(target.id, &engine).await.unwrap().state,
        ItemState::Queued
    );

    engine.reset_download(target.id).await;
    assert_eq!(
        snapshot_item(target.id, &engine).await.unwrap().state,
        ItemState::Queued
    );

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_download_preserves_a_stopped_items_scheduling_state() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = a_bin().with_state(ItemState::Stopped);
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    assert_eq!(
        snapshot_item(item.id, &engine).await.unwrap().state,
        ItemState::Stopped
    );

    engine.reset_download(item.id).await;

    assert_eq!(
        snapshot_item(item.id, &engine).await.unwrap().state,
        ItemState::Stopped
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_download_on_a_disabled_item_stays_stopped() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = a_bin().with_state(ItemState::Queued).with_enabled(false);
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;

    engine.reset_download(item.id).await;

    let reset = snapshot_item(item.id, &engine).await.unwrap();
    assert_eq!(reset.state, ItemState::Stopped);
    assert!(!reset.is_enabled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reorder_packages_applies_the_given_order() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let package_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    for (index, id) in package_ids.iter().enumerate() {
        engine
            .add(DownloadPackage::new(format!("P{index}")).with_id(*id))
            .await;
    }

    let new_order = [package_ids[2], package_ids[0], package_ids[1]];
    engine.reorder_packages(&new_order).await;

    let ordered: Vec<Uuid> = engine
        .snapshot()
        .await
        .packages
        .iter()
        .map(|package| package.id)
        .collect();
    assert_eq!(ordered, new_order);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adding_a_package_after_a_reorder_does_not_jump_ahead_in_scheduling_order() {
    let dir = make_scratch_directory();
    // Gated so whichever item is running stays running: the assertion below
    // is about scheduling order, not about how fast a transfer completes.
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(20_000), gate.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("First")
                .with_id(first_id)
                .with_items(vec![a_bin()]),
        )
        .await;
    engine
        .add(
            DownloadPackage::new("Second")
                .with_id(second_id)
                .with_items(vec![DownloadItem::http(test_source(), "b.bin")]),
        )
        .await;
    // Establishes real, distinct positions the same way a drag-and-drop
    // reorder would — `second_id` ends up with `position == 1`.
    engine.reorder_packages(&[first_id, second_id]).await;

    // A freshly constructed `DownloadPackage` defaults `position` to `0`,
    // same as every real call site — this must not collide with
    // `second_id`'s `position == 1` and jump ahead of it in scheduling order.
    let third_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("Third")
                .with_id(third_id)
                .with_items(vec![DownloadItem::http(test_source(), "c.bin")]),
        )
        .await;

    // `snapshot()` exposes array order, which is what the UI shows — confirm
    // it still matches insertion order.
    let packages = engine.snapshot().await.packages;
    assert_eq!(
        packages
            .iter()
            .map(|package| package.id)
            .collect::<Vec<_>>(),
        [first_id, second_id, third_id]
    );

    // Stop `first`'s item so its slot frees up, then tick once: with
    // `max_concurrent: 1`, the scheduler must pick `second`'s item next — not
    // `third`'s, which is what the position collision caused.
    let first_item_id = packages[0].items[0].id;
    engine.stop_item(first_item_id).await;
    engine.tick().await;

    let running: Vec<String> = engine
        .snapshot()
        .await
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .filter(|item| item.state == ItemState::Running)
        .map(|item| item.filename.clone())
        .collect();
    assert_eq!(running, ["b.bin"]);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_missing_is_true_only_when_a_completed_items_file_is_gone() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()).with_file_check_interval_ticks(1),
    );
    let item = a_bin();
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    engine.tick().await;
    assert!(!snapshot_item(item.id, &engine).await.unwrap().file_missing);

    let destination = dir.path().join("Batch").join("a.bin");
    std::fs::remove_file(&destination).unwrap();

    // The file is checked on the tick, not inside `snapshot()`.
    engine.tick().await;
    assert!(snapshot_item(item.id, &engine).await.unwrap().file_missing);
}

/// `pause_all()`/`resume_all()` are defined as "select every item and click
/// Stop/Start" — nothing more. In particular neither touches `is_enabled`, so
/// a user-disabled item stays stopped through a full pause/resume cycle
/// (Resume All is not a backdoor around Disable) while an enabled sibling gets
/// queued again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_all_and_resume_all_never_touch_is_enabled() {
    let dir = make_scratch_directory();
    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(10),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        sdm_engine::RetryPolicy::new(),
    );
    let items: Vec<DownloadItem> = (0..2)
        .map(|index| example_item(&format!("{index}.bin")))
        .collect();
    engine
        .add(DownloadPackage::new("Batch").with_items(items.clone()))
        .await;
    gate.wait_for_arrival().await;

    // One item explicitly disabled by the user before the blanket pause.
    engine.set_enabled(false, items[1].id).await;

    engine.pause_all().await;
    let snapshot = engine.snapshot().await;
    assert!(
        snapshot.packages[0]
            .items
            .iter()
            .all(|item| item.state == ItemState::Stopped)
    );
    assert!(snapshot.packages[0].items[0].is_enabled);
    assert!(!snapshot.packages[0].items[1].is_enabled);

    engine.resume_all().await;
    let snapshot = engine.snapshot().await;
    // The enabled item is queued again; the disabled one stays stopped —
    // Resume All is not a backdoor around Disable.
    assert_resumed(&snapshot.packages[0].items[0].state);
    assert_eq!(snapshot.packages[0].items[1].state, ItemState::Stopped);
    assert!(!snapshot.packages[0].items[1].is_enabled);

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

/// Disabling a running item stops it immediately (not merely marks it
/// ineligible for the next reconcile), and re-enabling it does not itself
/// resume it — the user still has to `start_item`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabling_a_running_item_stops_it_and_enabling_alone_does_not_resume_it() {
    let dir = make_scratch_directory();
    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(1000),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        sdm_engine::RetryPolicy::new(),
    );
    let package = DownloadPackage::new("Batch").with_items(vec![example_item("a.bin")]);
    let item_id = package.items[0].id;
    engine.add(package).await;
    gate.wait_for_arrival().await;

    // Disabling returns immediately — it does not wait for the gated worker
    // to actually unwind (that would hang for as long as the gate stays
    // closed, defeating the point of a user-facing Stop). The item's `state`
    // still flips to `Stopped` synchronously either way.
    engine.set_enabled(false, item_id).await;
    assert_eq!(
        snapshot_item(item_id, &engine).await.unwrap().state,
        ItemState::Stopped
    );

    engine.set_enabled(true, item_id).await;
    assert_eq!(
        snapshot_item(item_id, &engine).await.unwrap().state,
        ItemState::Stopped
    );

    engine.start_item(item_id).await;
    assert_resumed(&snapshot_item(item_id, &engine).await.unwrap().state);

    // Depending on whether the cancelled runner has unwound yet, the item is
    // still `Queued` behind it or already `Running` again (a cancelled
    // worker drops its parked fetch at once; see `assert_resumed`). Either
    // way, opening the gate lets whatever is running finish, and
    // `run_until_idle()` drains the old runner, reconciles again, and runs
    // the fresh attempt to completion.
    gate.open();
    within(engine.run_until_idle()).await.unwrap();
    assert_eq!(
        snapshot_item(item_id, &engine).await.unwrap().state,
        ItemState::Completed
    );
    assert_eq!(running_count(&engine).await, 0);
}

// MARK: - Beyond the Swift suite

/// An item is removed right after the user stopped it, while the gate still
/// holds its download. Whether the stopped runner has fully unwound by then is
/// up to the engine (a cancelled worker drops its parked fetch at once);
/// either way nothing of the item may survive the removal: no list entry, no
/// late landing written back onto it, no file. (The guarantee that the removal
/// waits for the runner is pinned by `removal_race_tests.rs`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_an_item_right_after_stopping_it_leaves_nothing_behind() {
    let dir = make_scratch_directory();
    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(1000),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        sdm_engine::RetryPolicy::new(),
    );
    let package = DownloadPackage::new("Batch").with_items(vec![example_item("a.bin")]);
    let item_id = package.items[0].id;
    engine.add(package).await;
    gate.wait_for_arrival().await;

    engine.stop_item(item_id).await;
    tokio::join!(within(engine.remove_item(item_id, true)), async {
        tokio::task::yield_now().await;
        gate.open();
    });

    assert!(engine.snapshot().await.packages.is_empty());
    within(engine.run_until_idle()).await.unwrap();
    assert!(engine.snapshot().await.packages.is_empty());
    assert!(!dir.path().join("Batch").join("a.bin").exists());
    assert!(!dir.path().join("Batch").join("a.bin.incomplete").exists());
}

/// The same for a reset right after a stop: none of the old run's bytes
/// survive, whether or not its runner had unwound when the reset began.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resetting_an_item_right_after_stopping_it_discards_its_progress() {
    let dir = make_scratch_directory();
    let gate = Gate::new();
    let engine = make_gated_engine(
        test_payload(1000),
        dir.path(),
        &gate,
        GatePhase::Body,
        false,
        sdm_engine::RetryPolicy::new(),
    );
    let package = DownloadPackage::new("Batch").with_items(vec![example_item("a.bin")]);
    let item_id = package.items[0].id;
    engine.add(package).await;
    gate.wait_for_arrival().await;

    engine.stop_item(item_id).await;
    tokio::join!(within(engine.reset_download(item_id)), async {
        tokio::task::yield_now().await;
        gate.open();
    });

    let reset = snapshot_item(item_id, &engine).await.unwrap();
    assert_eq!(reset.state, ItemState::Stopped);
    assert_eq!(reset.completed.total_bytes(), 0);
    assert_eq!(reset.total_bytes, None);
    assert_eq!(running_count(&engine).await, 0);
}

/// Naming an item twice moves it once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_ignores_a_repeated_id() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let items: Vec<DownloadItem> = (0..2)
        .map(|index| example_item(&format!("{index}.bin")).with_enabled(false))
        .collect();
    let (source, destination) = (Uuid::new_v4(), Uuid::new_v4());
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(source)
                .with_items(items.clone()),
        )
        .await;
    engine
        .add(DownloadPackage::new("B").with_id(destination))
        .await;

    engine
        .move_items_to_package(&[items[0].id, items[0].id], destination, None)
        .await;

    let packages = engine.snapshot().await.packages;
    assert_eq!(packages[0].items.len(), 1);
    assert_eq!(packages[0].items[0].id, items[1].id);
    assert_eq!(packages[1].items.len(), 1);
    assert_eq!(packages[1].items[0].id, items[0].id);
}

/// Moving items into a new package names it, puts them first, and drops the
/// package they left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_to_new_package_creates_it_and_drops_the_emptied_source() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = example_item("a.bin").with_enabled(false);
    let source = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(source)
                .with_items(vec![item.clone()]),
        )
        .await;

    engine.move_items_to_new_package(&[item.id], "Fresh").await;

    let packages = engine.snapshot().await.packages;
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "Fresh");
    assert_eq!(packages[0].items[0].id, item.id);
}
