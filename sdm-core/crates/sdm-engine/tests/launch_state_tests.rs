//! Port of Swift's `LaunchStateTests.swift` (3 tests).
//!
//! "Resume downloads automatically on launch" used to be its own in-memory
//! suspension flag (`isGloballySuspended`), because the old single-axis model
//! had no real `Stopped` state to durably rest on — the only way to hold an
//! item back was `is_enabled`, which belongs to the user. Now that `Stopped`
//! is a first-class, correctly-persisted state, the durable store never
//! claims anything is in flight or about to run, and "auto start on launch"
//! is nothing but `resume_all()` called once after `restore()`.
mod support;

use std::sync::Arc;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, SourceUrl};
use sdm_engine::{FakeOrigin, InMemoryStateStore, PersistedState, StateStore};
use support::{
    Gate, GatePhase, GatedOrigin, engine_settings, make_scratch_directory, new_engine_with_store,
    snapshot_item, test_payload, within,
};

fn item(name: &str) -> DownloadItem {
    DownloadItem::http(
        SourceUrl::new(format!("https://example.com/{name}")).unwrap(),
        name,
    )
}

/// `persist()` squashes `Queued`/`Running` to `Stopped` in what actually
/// reaches the store, even while the live engine reports the real state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persisted_snapshot_never_contains_queued_or_running() {
    let dir = make_scratch_directory();
    let store = Arc::new(InMemoryStateStore::new());
    let gate = Gate::new();
    let engine = new_engine_with_store(
        GatedOrigin::new(test_payload(4000), gate.clone(), GatePhase::Body, false),
        store.clone(),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = item("a.bin");
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    gate.wait_for_arrival().await;

    // The live engine genuinely has it running...
    assert_eq!(
        snapshot_item(item.id, &engine).await.unwrap().state,
        ItemState::Running
    );
    // ...but nothing written to the store ever says so.
    assert_eq!(
        store.load().await.packages[0].items[0].state,
        ItemState::Stopped
    );

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

/// A store seeded directly (as a crash or an older build might leave one)
/// with `Queued`/`Running` items is normalized to `Stopped` on restore, same
/// as `persist()` would have written it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restore_normalizes_legacy_queued_and_running_to_stopped() {
    let dir = make_scratch_directory();
    let package = DownloadPackage::new("Batch").with_items(vec![
        item("a.bin").with_state(ItemState::Running),
        item("b.bin").with_state(ItemState::Queued),
    ]);

    let store = Arc::new(InMemoryStateStore::new());
    store.save(PersistedState::new(vec![package])).await;

    let engine = new_engine_with_store(
        FakeOrigin::new(test_payload(10)),
        store,
        engine_settings(1, 1, 8, dir.path()),
    );
    engine.restore().await;

    let restored = engine.snapshot().await.packages[0].items.clone();
    assert_eq!(restored.len(), 2);
    assert!(restored.iter().all(|item| item.state == ItemState::Stopped));
}

/// The whole point of the redesign: "resume on launch" is just `resume_all()`
/// after `restore()`, and it respects the same enabled/disabled split every
/// other Resume All call does — no separate suspension mechanism, no magic
/// "starting one item resumes everyone."
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_all_after_restore_reproduces_auto_start_on_launch() {
    let dir = make_scratch_directory();
    let package = DownloadPackage::new("Batch").with_items(vec![
        item("a.bin").with_state(ItemState::Stopped),
        item("b.bin")
            .with_state(ItemState::Stopped)
            .with_enabled(false),
    ]);
    let (first, second) = (package.items[0].id, package.items[1].id);

    let store = Arc::new(InMemoryStateStore::new());
    store.save(PersistedState::new(vec![package])).await;

    let engine = new_engine_with_store(
        FakeOrigin::new(test_payload(10)),
        store,
        engine_settings(2, 1, 8, dir.path()),
    );
    engine.restore().await;

    // Auto-start off: nothing here calls resume_all, so both items sit put.
    for _ in 0..3 {
        engine.tick().await;
    }
    assert_eq!(
        snapshot_item(first, &engine).await.unwrap().state,
        ItemState::Stopped
    );
    assert_eq!(
        snapshot_item(second, &engine).await.unwrap().state,
        ItemState::Stopped
    );

    // Auto-start on: the app layer's equivalent is one resume_all() call.
    engine.resume_all().await;
    within(engine.run_until_idle()).await.unwrap();

    assert_eq!(
        snapshot_item(first, &engine).await.unwrap().state,
        ItemState::Completed
    );
    // Disabled item never resumed, even though Resume All ran.
    let disabled = snapshot_item(second, &engine).await.unwrap();
    assert_eq!(disabled.state, ItemState::Stopped);
    assert!(!disabled.is_enabled);
}
