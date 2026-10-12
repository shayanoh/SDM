//! Port of Swift's `DurableStateTests.swift` (5 tests).
//!
//! Spec §4.2: durable state is written "debounced (~2 s after the last
//! change), plus on quit".
//!
//! `JsonStateStore::save()` only stores into `pending` in memory; `flush()` is
//! the sole writer of `state.json`. Its only caller anywhere used to be
//! `shutdown()`, which runs only if the app gets the chance to call it — and
//! on a quit it does not. So in the ordinary case *nothing was ever written*:
//! the `.incomplete` file and its `.sdmpart` survived with no record of the
//! item owning them, and the next launch restored an empty list.
mod support;

use std::path::Path;
use std::sync::Arc;

use sdm_core::{ComponentOrigin, DownloadItem, DownloadPackage, ItemState, SourceUrl};
use sdm_engine::{
    DownloadEngine, FakeOrigin, FakeOriginBehavior, JsonStateStore, PersistedState, ResumeSidecar,
    SparseFile, StateStore,
};
use support::{
    engine_settings, make_scratch_directory, new_engine_with_store, test_payload, test_source,
    within,
};
use uuid::Uuid;

fn stopped_package(name: &str) -> DownloadPackage {
    DownloadPackage::new("Batch").with_items(vec![
        DownloadItem::http(
            SourceUrl::new(format!("https://example.com/{name}")).unwrap(),
            name,
        )
        .with_enabled(false),
    ])
}

fn json_engine(dir: &Path, state_url: &Path, debounce: usize) -> DownloadEngine {
    new_engine_with_store(
        FakeOrigin::new(test_payload(100)),
        Arc::new(JsonStateStore::new(state_url)),
        engine_settings(1, 1, 8, dir).with_persist_debounce_ticks(debounce),
    )
}

async fn item_of(engine: &DownloadEngine, id: Uuid) -> Option<sdm_engine::ItemSnapshot> {
    support::snapshot_item(id, engine).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn heartbeat_writes_durable_state_without_any_shutdown() {
    let dir = make_scratch_directory();
    let state_url = dir.path().join("state.json");
    let engine = json_engine(dir.path(), &state_url, 2);

    engine.add(stopped_package("a.bin")).await;
    // A save alone is in-memory only; nothing has been written yet.
    assert!(!state_url.exists());

    engine.tick().await;
    assert!(!state_url.exists());

    // Second tick crosses the debounce window. No shutdown() anywhere.
    engine.tick().await;
    assert!(state_url.exists());

    let reread = JsonStateStore::new(&state_url).load().await;
    let names: Vec<&str> = reread
        .packages
        .iter()
        .flat_map(|package| &package.items)
        .map(|item| item.filename())
        .collect();
    assert_eq!(names, ["a.bin"]);
}

/// The debounce must be cheap when idle: a tick with nothing dirty must not
/// re-encode and re-write the snapshot every second forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_quiescent_engine_stops_writing_durable_state() {
    let dir = make_scratch_directory();
    let state_url = dir.path().join("state.json");
    let engine = json_engine(dir.path(), &state_url, 2);

    engine.add(stopped_package("a.bin")).await;
    engine.tick().await;
    engine.tick().await;
    assert!(state_url.exists());

    std::fs::remove_file(&state_url).unwrap();
    for _ in 0..10 {
        engine.tick().await;
    }
    assert!(!state_url.exists());
}

// MARK: - The real end-to-end restart

/// The completion criterion, tested for real: "Quitting mid-download and
/// relaunching resumes rather than restarting."
///
/// Nothing here is simulated. A `JsonStateStore` over an actual file, a real
/// `.incomplete` file and `.sdmpart` sidecar in a real download folder, a
/// download taken partway by an origin that truncates, a `shutdown()`, then a
/// *completely fresh* engine constructed over the same store and folder.
///
/// The second engine gets its own `FakeOrigin`, so its `requested_ranges` is
/// a clean record of what the resumed run actually asked for — which is how
/// "did not restart" is proved rather than asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quitting_mid_download_and_relaunching_resumes_rather_than_restarting() {
    let dir = make_scratch_directory();
    let state_url = dir.path().join("state.json");
    let payload = test_payload(4000);
    let prefix: i64 = 2000;

    let package =
        DownloadPackage::new("Batch").with_items(vec![DownloadItem::http(test_source(), "a.bin")]);
    let item_id = package.items[0].id;
    let destination = dir.path().join("Batch").join("a.bin");

    // --- Launch 1: gets 2000 of 4000 bytes, then the origin truncates. ---
    let first_engine = new_engine_with_store(
        FakeOrigin::with_behavior(
            payload.clone(),
            FakeOriginBehavior {
                truncate_after_bytes: Some(prefix as usize),
                chunk_size: 250,
                ..Default::default()
            },
        ),
        Arc::new(JsonStateStore::new(&state_url)),
        engine_settings(1, 1, 8, dir.path()).with_checkpoint_interval_bytes(500),
    );
    first_engine.add(package).await;
    within(first_engine.run_until_idle()).await.unwrap();

    let midway = item_of(&first_engine, item_id).await.unwrap();
    assert_eq!(midway.completed.total_bytes(), prefix);

    // Quit.
    within(first_engine.shutdown()).await;

    // The three things a relaunch depends on all actually exist on disk.
    assert!(state_url.exists());
    assert!(SparseFile::incomplete_path(&destination).exists());
    let sidecar = ResumeSidecar::load(&ResumeSidecar::path_for(&destination))
        .expect("the sidecar was written");
    assert_eq!(sidecar.completed.total_bytes(), prefix);
    assert!(!destination.exists());

    // What actually landed in state.json — the item, and its progress.
    let persisted = JsonStateStore::new(&state_url).load().await;
    let persisted_item = persisted.packages.iter().flat_map(|p| &p.items).next();
    let persisted_item = persisted_item.expect("the item was persisted");
    assert_eq!(persisted_item.id, item_id);
    assert_eq!(persisted_item.filename(), "a.bin");
    assert_eq!(persisted_item.completed().total_bytes(), prefix);
    assert_eq!(persisted_item.total_bytes(), Some(4000));

    // --- Launch 2: a brand new engine over the same store and folder. ---
    let second_origin = FakeOrigin::new(payload.clone());
    let second_engine = new_engine_with_store(
        second_origin.clone(),
        Arc::new(JsonStateStore::new(&state_url)),
        engine_settings(1, 1, 8, dir.path()).with_checkpoint_interval_bytes(500),
    );
    second_engine.restore().await;

    // The item came back from the JSON file, landed `Stopped` (restore never
    // auto-resumes — see `DownloadEngine::restore`'s doc comment). The
    // persisted figure was checked above; what the resumed run does with it
    // is proved below, by what it asks the origin for.
    let restored = item_of(&second_engine, item_id).await.unwrap();
    assert_eq!(restored.filename, "a.bin");
    assert_eq!(restored.state, ItemState::Stopped);

    // "Resume downloads automatically on launch" is exactly this call, made
    // by `EngineController` in the real app — see `DownloadEngine::resume_all`.
    second_engine.resume_all().await;
    within(second_engine.run_until_idle()).await.unwrap();

    // Byte identity.
    assert_eq!(std::fs::read(&destination).unwrap(), payload);
    assert_eq!(
        item_of(&second_engine, item_id).await.unwrap().state,
        ItemState::Completed
    );

    // Resumed, not restarted: apart from `prepare()`'s one-byte probe, the
    // second run never asked for a byte it already had.
    let body_requests: Vec<_> = second_origin
        .requested_ranges()
        .into_iter()
        .filter(|range| range.length() > 1)
        .collect();
    assert!(!body_requests.is_empty());
    for request in body_requests {
        assert!(request.start >= prefix, "re-requested {request:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loads_and_migrates_a_v1_snapshot_to_multi_component_items() {
    let dir = make_scratch_directory();
    let file_url = dir.path().join("state.json");
    let v1 = r#"
        {
          "formatVersion": 1,
          "packages": [
            {
              "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
              "name": "Old Package",
              "priority": 2,
              "position": 0,
              "items": [
                {
                  "id": "2B4E28BA-2FA1-11D2-883F-0016D3CCA427",
                  "url": "https://example.com/big.iso",
                  "filename": "big.iso",
                  "totalBytes": 5000,
                  "completed": { "ranges": [ { "start": 0, "end": 2000 } ] },
                  "state": { "stopped": {} },
                  "isEnabled": true,
                  "isResumable": true,
                  "position": 0
                }
              ]
            }
          ]
        }
        "#;
    std::fs::write(&file_url, v1).unwrap();

    let store = JsonStateStore::new(&file_url);
    let loaded = store.load().await;
    assert_eq!(loaded.format_version, 2);
    assert_eq!(loaded.packages.len(), 1);
    let item = &loaded.packages[0].items[0];
    assert_eq!(item.components.len(), 1);
    assert_eq!(item.components[0].origin, ComponentOrigin::Http);
    assert_eq!(item.url().as_str(), "https://example.com/big.iso");
    assert_eq!(item.completed().total_bytes(), 2000);

    store.save(loaded).await;
    store.flush().await;
    let reread: PersistedState =
        serde_json::from_slice(&std::fs::read(&file_url).unwrap()).unwrap();
    assert_eq!(reread.format_version, 2);
    assert_eq!(reread.packages[0].items[0].components.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_restores_a_v2_model_item_and_reports_it_complete() {
    let dir = make_scratch_directory();
    let state_url = dir.path().join("state.json");
    let payload = test_payload(4096);
    let settings = engine_settings(2, 4, 16, dir.path());
    let source = SourceUrl::new("https://example.com/out.bin").unwrap();

    {
        let engine = new_engine_with_store(
            FakeOrigin::new(payload.clone()),
            Arc::new(JsonStateStore::new(&state_url)),
            settings.clone(),
        );
        engine
            .add(DownloadPackage::new("Pkg").with_items(vec![
                DownloadItem::http(source, "out.bin").with_state(ItemState::Queued),
            ]))
            .await;
        within(engine.run_until_idle()).await.unwrap();
        within(engine.shutdown()).await;
    }

    let engine2 = new_engine_with_store(
        FakeOrigin::new(payload),
        Arc::new(JsonStateStore::new(&state_url)),
        settings,
    );
    engine2.restore().await;
    let item = engine2.snapshot().await.packages[0].items[0].clone();
    assert_eq!(item.state, ItemState::Completed);

    let reread: PersistedState =
        serde_json::from_slice(&std::fs::read(&state_url).unwrap()).unwrap();
    assert_eq!(reread.format_version, 2);
    assert_eq!(reread.packages[0].items[0].components.len(), 1);
}
