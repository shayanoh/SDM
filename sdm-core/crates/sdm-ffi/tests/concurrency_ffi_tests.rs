//! Review Focus 5: calling into the engine from Swift while a tick is in
//! flight neither deadlocks nor panics across the FFI boundary.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdm_core::{DownloadItem, DownloadPackage};
use sdm_engine::{FakeOrigin, PersistedState, RetryPolicy, StateStore, Trash};
use sdm_ffi::shared::lock;
use sdm_ffi::{CoreError, DownloadEngine};
use support::{
    ScratchTrash, dependencies, engine_over, engine_settings, package, payload, scratch_directory,
    su,
};
use uuid::Uuid;

const TASKS: usize = 16;
const CALLS_PER_TASK: usize = 2_000;
const MAX_TRACKED_ITEMS: usize = 120;

/// A deterministic pseudo-random generator (SplitMix64), so a failing run can
/// be reproduced from the seed printed on failure.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// The items and packages the hammering tasks know about, so they can aim
/// their calls at ids that exist as well as ones that no longer do.
#[derive(Default)]
struct Known {
    items: Vec<Uuid>,
    packages: Vec<Uuid>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn sixteen_tasks_hammer_one_engine_while_another_shuts_it_down() {
    let seed = 0x5EED_0023_u64;
    let dir = scratch_directory();
    let (engine, _trash) = engine_over(payload(1024), dir.path());
    let known = Arc::new(Mutex::new(Known::default()));
    let completed_calls = Arc::new(AtomicUsize::new(0));
    let panics = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(Mutex::new(Vec::<CoreError>::new()));

    let work = async {
        let mut handles = Vec::new();
        for task in 0..TASKS {
            let engine = Arc::clone(&engine);
            let known = Arc::clone(&known);
            let completed_calls = Arc::clone(&completed_calls);
            let errors = Arc::clone(&errors);
            let panics = Arc::clone(&panics);
            handles.push(tokio::spawn(async move {
                let mut rng = Rng(seed ^ (task as u64 + 1).wrapping_mul(0x1234_5678_9ABC_DEF1));
                for call in 0..CALLS_PER_TASK {
                    let pick_item = |rng: &mut Rng| {
                        let known = known.lock().unwrap();
                        if known.items.is_empty() || rng.below(5) == 0 {
                            Uuid::new_v4()
                        } else {
                            known.items[rng.below(known.items.len())]
                        }
                    };
                    let result = match rng.below(8) {
                        0 | 1 => engine.tick().await,
                        2 | 3 => engine.snapshot().await.map(|_| ()),
                        4 => {
                            let id = pick_item(&mut rng);
                            let enabled = rng.below(2) == 0;
                            engine.set_enabled(enabled, id).await
                        }
                        5 => {
                            let (mut ids, package_id) = {
                                let known = known.lock().unwrap();
                                let package_id = if known.packages.is_empty() {
                                    Uuid::new_v4()
                                } else {
                                    known.packages[rng.below(known.packages.len())]
                                };
                                (known.items.clone(), package_id)
                            };
                            // A shuffle of whatever is known, truncated.
                            for i in (1..ids.len()).rev() {
                                ids.swap(i, rng.below(i + 1));
                            }
                            ids.truncate(8);
                            engine.reorder_items(ids, package_id).await
                        }
                        6 => {
                            let id = pick_item(&mut rng);
                            if rng.below(4) == 0 {
                                let mut known = known.lock().unwrap();
                                known.items.retain(|known_id| *known_id != id);
                            }
                            engine.remove_item(id, false).await
                        }
                        _ => {
                            // add: a fresh item into one of a few packages.
                            let room = known.lock().unwrap().items.len() < MAX_TRACKED_ITEMS;
                            if room && call % 4 == 0 {
                                let file = format!("t{task}-c{call}.bin");
                                let item = DownloadItem::http(
                                    su(&format!("https://example.com/{file}")),
                                    file,
                                );
                                let item_id = item.id;
                                let package = DownloadPackage::new(format!("Pkg{}", task % 3))
                                    .with_items(vec![item]);
                                let result = engine.add(package).await;
                                if result.is_ok() {
                                    known.lock().unwrap().items.push(item_id);
                                }
                                result
                            } else {
                                engine.tick().await
                            }
                        }
                    };
                    if let Err(error) = result {
                        if matches!(error, CoreError::Panicked { .. }) {
                            panics.fetch_add(1, Ordering::SeqCst);
                        }
                        errors.lock().unwrap().push(error);
                    }
                    completed_calls.fetch_add(1, Ordering::SeqCst);
                    // Let the other tasks, the engine's jobs and the shutdown
                    // task get a turn between calls.
                    tokio::task::yield_now().await;
                }
            }));
        }

        // The seventeenth task: shuts the engine down at a random point of the
        // storm and then keeps calling, since Swift would.
        let shutdown_after = 2_000 + (Rng(seed).next() as usize) % (TASKS * CALLS_PER_TASK / 2);
        let shutdown_engine = Arc::clone(&engine);
        let shutdown_progress = Arc::clone(&completed_calls);
        let shutdown_errors = Arc::clone(&errors);
        handles.push(tokio::spawn(async move {
            while shutdown_progress.load(Ordering::SeqCst) < shutdown_after {
                tokio::task::yield_now().await;
            }
            if let Err(error) = shutdown_engine.shutdown().await {
                shutdown_errors.lock().unwrap().push(error);
            }
            if let Err(error) = shutdown_engine.shutdown().await {
                shutdown_errors.lock().unwrap().push(error);
            }
        }));

        for handle in handles {
            handle.await.expect("a hammering task must not panic");
        }
    };

    tokio::time::timeout(Duration::from_secs(60), work)
        .await
        .expect("the engine deadlocked or stalled under concurrent calls (seed 0x5EED0023)");

    assert_eq!(panics.load(Ordering::SeqCst), 0, "no call may panic");
    {
        let errors = errors.lock().unwrap();
        assert!(errors.is_empty(), "no call may fail: {errors:?}");
    }
    assert_eq!(
        completed_calls.load(Ordering::SeqCst),
        TASKS * CALLS_PER_TASK,
        "every call returned"
    );

    // The engine is still answering after the storm and the shutdown.
    engine.snapshot().await.expect("snapshot after the storm");
    engine.shutdown().await.expect("a further shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panicking_call_is_reported_as_an_error_not_an_abort() {
    let dir = scratch_directory();
    let (engine, _trash) = engine_over(payload(16), dir.path());

    let asynchronous = engine.panic_for_testing("boom (async)".to_owned()).await;
    match asynchronous {
        Err(CoreError::Panicked { message }) => assert!(message.contains("boom (async)")),
        other => panic!("expected a Panicked error, got {other:?}"),
    }
    let synchronous = engine.panic_synchronously_for_testing("boom (sync)".to_owned());
    match synchronous {
        Err(CoreError::Panicked { message }) => assert!(message.contains("boom (sync)")),
        other => panic!("expected a Panicked error, got {other:?}"),
    }

    // The process is alive and the engine still works.
    engine
        .add(package("After", "after.bin", false))
        .await
        .unwrap();
    assert_eq!(engine.snapshot().await.unwrap().packages.len(), 1);
}

/// A state store whose `save` panics, so every mutating engine call that
/// persists panics inside the engine, after it changed its state.
struct PanickingStore {
    armed: AtomicBool,
}

#[async_trait]
impl StateStore for PanickingStore {
    async fn load(&self) -> PersistedState {
        PersistedState::default()
    }

    async fn save(&self, _state: PersistedState) {
        if self.armed.load(Ordering::SeqCst) {
            panic!("the state store exploded");
        }
    }

    async fn flush(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_inside_the_engine_surfaces_as_an_error_and_later_calls_still_work() {
    let dir = scratch_directory();
    let store = Arc::new(PanickingStore {
        armed: AtomicBool::new(true),
    });
    let engine = DownloadEngine::with_dependencies(
        engine_settings(dir.path()),
        dependencies(
            FakeOrigin::new(payload(16)),
            store.clone(),
            Arc::new(ScratchTrash::new()),
        ),
        DownloadEngine::default_tools(),
    )
    .unwrap();

    // The panic happens inside the engine's `add`, after its state changed.
    let result = engine.add(package("Boom", "boom.bin", false)).await;
    match result {
        Err(CoreError::Panicked { message }) => {
            assert!(message.contains("exploded"), "{message}")
        }
        other => panic!("expected a Panicked error, got {other:?}"),
    }

    // Reads and ticks keep answering: nothing is left locked or poisoned in a
    // way that stops them.
    let snapshot = engine.snapshot().await.expect("snapshot after a panic");
    assert_eq!(
        snapshot.packages.len(),
        1,
        "the add took effect before the panic"
    );
    engine.tick().await.expect("tick after a panic");

    // Disarm the store: the engine works normally again.
    store.armed.store(false, Ordering::SeqCst);
    engine
        .add(package("Fine", "fine.bin", false))
        .await
        .unwrap();
    assert_eq!(engine.snapshot().await.unwrap().packages.len(), 2);
}

/// A trash that panics, called by `remove_item(delete_file: true)`.
struct PanickingTrash;

impl Trash for PanickingTrash {
    fn trash(&self, _path: &Path) -> std::io::Result<()> {
        panic!("the trash exploded");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_while_removing_files_does_not_poison_the_engine() {
    let dir = scratch_directory();
    let engine = DownloadEngine::with_dependencies(
        engine_settings(dir.path()),
        {
            let mut deps = dependencies(
                FakeOrigin::new(payload(16)),
                Arc::new(sdm_engine::InMemoryStateStore::new()),
                Arc::new(PanickingTrash),
            );
            deps.retry_policy = RetryPolicy::new();
            deps
        },
        DownloadEngine::default_tools(),
    )
    .unwrap();

    // A file on disk for the item, so removal has something to trash.
    engine
        .add(package("Doomed", "doomed.bin", false))
        .await
        .unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    let id = snapshot.packages[0].items[0].id;
    let folder = dir.path().join("Doomed");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("doomed.bin"), b"data").unwrap();

    let _ = engine.remove_item(id, true).await; // Ok or Panicked: both are errors-as-values
    engine.snapshot().await.expect("snapshot still answers");
    engine.tick().await.expect("tick still answers");
    engine
        .add(package("Next", "next.bin", false))
        .await
        .unwrap();
    assert!(
        engine
            .snapshot()
            .await
            .unwrap()
            .packages
            .iter()
            .any(|p| p.name == "Next")
    );
}

#[test]
fn a_poisoned_settings_mutex_is_recovered_not_propagated() {
    let mutex = Arc::new(Mutex::new(vec!["a".to_owned()]));
    let poisoner = Arc::clone(&mutex);
    let result = std::thread::spawn(move || {
        let _guard = poisoner.lock().unwrap();
        panic!("poison the mutex");
    })
    .join();
    assert!(result.is_err());
    assert!(mutex.is_poisoned());
    // `lock` hands the value back instead of panicking.
    assert_eq!(*lock(&mutex), vec!["a".to_owned()]);
}
