use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use sdm_core::{
    ByteRange, DownloadItem, DownloadPackage, ItemState, Priority, RangeSet, SourceUrl,
};
use sdm_engine::{InMemoryStateStore, JsonStateStore, PersistedState, StateStore};
use serde_json::{Value, json};

fn url(string: &str) -> SourceUrl {
    SourceUrl::new(string).unwrap()
}

fn sample_package(name: &str) -> DownloadPackage {
    DownloadPackage::new(name).with_items(vec![DownloadItem::http(
        url("https://example.com/a.bin"),
        "a.bin",
    )])
}

fn names(state: &PersistedState) -> Vec<String> {
    state.packages.iter().map(|p| p.name.clone()).collect()
}

fn fixture(name: &str) -> String {
    let path = format!("{}/../../fixtures/swift/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loading_from_empty_directory_returns_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonStateStore::new(dir.path().join("state.json"));
    assert!(store.load().await.packages.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saved_state_round_trips_after_flush() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let store = JsonStateStore::new(&path);
    store
        .save(PersistedState::new(vec![sample_package("Season 1")]))
        .await;
    store.flush().await;

    let reopened = JsonStateStore::new(&path);
    assert_eq!(names(&reopened.load().await), ["Season 1"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn save_without_flush_does_not_touch_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let store = JsonStateStore::new(&path);
    store
        .save(PersistedState::new(vec![sample_package("Season 1")]))
        .await;
    assert!(!path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeated_saves_coalesce_to_the_latest_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let store = JsonStateStore::new(&path);
    for name in ["first", "second", "third"] {
        store
            .save(PersistedState::new(vec![sample_package(name)]))
            .await;
    }
    store.flush().await;

    assert_eq!(names(&JsonStateStore::new(&path).load().await), ["third"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flush_with_nothing_pending_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonStateStore::new(dir.path().join("state.json"));
    store.flush().await;
    store.flush().await;
    assert!(store.load().await.packages.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_state_file_loads_as_empty_rather_than_crashing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(JsonStateStore::new(&path).load().await.packages.is_empty());
    // The bad file is left where it is.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_format_version_loads_as_empty_and_is_left_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let text = serde_json::to_string(&PersistedState {
        format_version: 3,
        packages: vec![sample_package("Season 1")],
    })
    .unwrap();
    std::fs::write(&path, &text).unwrap();

    assert!(JsonStateStore::new(&path).load().await.packages.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn in_memory_store_round_trips() {
    let store = InMemoryStateStore::new();
    store
        .save(PersistedState::new(vec![sample_package("Season 1")]))
        .await;
    store.flush().await;
    assert_eq!(names(&store.load().await), ["Season 1"]);
}

// MARK: - flush() failure handling
//
// `flush()` only clears `pending` once the write has actually succeeded; on
// any failure the save stays queued so the *next* `flush()` call retries it.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flush_preserves_pending_state_when_the_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    // Strip write permission from the directory so the atomic write (which
    // creates a temp file alongside the destination) fails with EACCES.
    let set_mode = |mode: u32| {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set_mode(0o500);

    let store = JsonStateStore::new(&path);
    store
        .save(PersistedState::new(vec![sample_package("first")]))
        .await;
    store.flush().await;

    // The write failed; nothing should have landed on disk, and the save
    // must not have been discarded.
    assert!(!path.exists());

    // Restore permission and flush again: the pending save from before
    // must still be there, waiting to retry.
    set_mode(0o755);
    store.flush().await;

    assert_eq!(names(&JsonStateStore::new(&path).load().await), ["first"]);
}

// MARK: - Round-trip fidelity

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn round_trip_preserves_full_package_graph_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let mut partially_downloaded =
        DownloadItem::http(url("https://example.com/movie.mkv"), "movie.mkv")
            .with_total_bytes(1_000)
            .with_enabled(true)
            .with_position(1)
            .with_state(ItemState::Running);
    partially_downloaded.set_is_resumable(Some(true));
    partially_downloaded.set_validator(Some("etag-abc123".to_string()));
    partially_downloaded.set_completed(RangeSet::from_ranges(&[
        ByteRange::new(0, 100),
        ByteRange::new(500, 750),
    ]));

    let mut disabled_item = DownloadItem::http(url("https://example.com/subs.srt"), "subs.srt")
        .with_enabled(false)
        .with_priority(Priority::Highest)
        .with_position(0);
    disabled_item.set_is_resumable(Some(false));

    let failed_item = DownloadItem::http(url("https://example.com/cover.jpg"), "cover.jpg")
        .with_state(ItemState::Failed {
            reason: "404 Not Found".to_string(),
        })
        .with_priority(Priority::Lowest)
        .with_position(2);

    let package = DownloadPackage::new("Season 1")
        // Deliberately not stored in position order, to prove ordering is
        // preserved as array order, not resorted on load.
        .with_items(vec![partially_downloaded, disabled_item, failed_item])
        .with_priority(Priority::High)
        .with_position(3);

    let store = JsonStateStore::new(&path);
    let saved = PersistedState::new(vec![package]);
    store.save(saved.clone()).await;
    store.flush().await;

    let reloaded = JsonStateStore::new(&path).load().await;
    assert_eq!(reloaded, saved);

    // Spell out the properties the equality check above is standing in for.
    let package = &reloaded.packages[0];
    assert_eq!(package.priority, Priority::High);
    assert_eq!(package.position, 3);
    let filenames: Vec<_> = package
        .items
        .iter()
        .map(|i| i.output_filename.as_str())
        .collect();
    assert_eq!(filenames, ["movie.mkv", "subs.srt", "cover.jpg"]);
    let positions: Vec<_> = package.items.iter().map(|i| i.position).collect();
    assert_eq!(positions, [1, 0, 2]);
    assert_eq!(package.items[0].priority, None);
    assert_eq!(package.items[1].priority, Some(Priority::Highest));
    assert_eq!(
        package.items[0].completed().ranges(),
        [ByteRange::new(0, 100), ByteRange::new(500, 750)]
    );
    assert_eq!(package.items[0].state, ItemState::Running);
    assert!(!package.items[1].is_enabled);
    assert_eq!(
        package.items[2].state,
        ItemState::Failed {
            reason: "404 Not Found".to_string()
        }
    );
    assert_eq!(package.items[0].validator(), Some("etag-abc123"));
}

// MARK: - Corrupt-snapshot validation
//
// Deserialization does not route through the package/item constructors, so
// their assertions never fire on a hand-edited or corrupt snapshot.
// Rejecting such a snapshot is this store's job: `load()` returns an empty
// `PersistedState`, never a value containing the invalid model.

/// Builds real, schema-correct JSON for a valid snapshot (by serializing
/// it), then corrupts one field by direct string substitution.
fn encoded_json(state: &PersistedState) -> String {
    serde_json::to_string(state).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_with_empty_package_name_loads_as_empty_rather_than_invalid_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let mut json = encoded_json(&PersistedState::new(vec![sample_package("Season 1")]));
    assert!(json.contains("\"Season 1\""));
    json = json.replace("\"Season 1\"", "\"\"");
    std::fs::write(&path, json).unwrap();

    assert!(JsonStateStore::new(&path).load().await.packages.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_with_empty_item_filename_loads_as_empty_rather_than_invalid_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let mut json = encoded_json(&PersistedState::new(vec![sample_package("Season 1")]));
    assert!(json.contains("\"a.bin\""));
    json = json.replace("\"a.bin\"", "\"\"");
    std::fs::write(&path, json).unwrap();

    assert!(JsonStateStore::new(&path).load().await.packages.is_empty());
}

/// Hand-written legacy (flat-shape, format version 1) JSON: keys in a
/// different order than the encoder emits, an item with no
/// totalBytes/validator/priority (all omitted, not null), and the violation
/// (empty filename) nested inside an otherwise entirely valid package.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hand_written_snapshot_with_empty_nested_item_filename_loads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let json = r#"
        {
          "packages": [
            {
              "priority": 2,
              "name": "Season 1",
              "id": "8B687F15-C739-421D-9E0F-1D2EFD8AC989",
              "position": 0,
              "items": [
                {
                  "state": { "queued": {} },
                  "url": "https://example.com/a.bin",
                  "isEnabled": true,
                  "filename": "",
                  "isResumable": false,
                  "id": "37B1C7A5-EDCF-4DA5-8849-FED5AC7AF237",
                  "completed": { "ranges": [] },
                  "position": 0
                }
              ]
            }
          ],
          "formatVersion": 1
        }
        "#;
    std::fs::write(&path, json).unwrap();

    assert!(JsonStateStore::new(&path).load().await.packages.is_empty());
}

/// `isResumable` widened from `Bool` to `Bool?` (`nil` = "not probed yet").
/// `load()` fails closed — any decode error returns an empty state — so a
/// regression here would silently wipe a user's entire download list.
/// Case 1: a snapshot carrying an explicit `"isResumable": false` / `true`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_with_literal_is_resumable_still_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let json = r#"
        {
          "formatVersion": 1,
          "packages": [
            {
              "id": "8B687F15-C739-421D-9E0F-1D2EFD8AC989",
              "name": "Season 1",
              "position": 0,
              "priority": 2,
              "items": [
                {
                  "id": "37B1C7A5-EDCF-4DA5-8849-FED5AC7AF237",
                  "url": "https://example.com/a.bin",
                  "filename": "a.bin",
                  "totalBytes": 100,
                  "completed": { "ranges": [ { "start": 0, "end": 10 } ] },
                  "state": { "queued": {} },
                  "isEnabled": true,
                  "isResumable": false,
                  "position": 0
                },
                {
                  "id": "1E1D0B4A-3A2C-4E2B-9C1D-2F3A4B5C6D7E",
                  "url": "https://example.com/b.bin",
                  "filename": "b.bin",
                  "completed": { "ranges": [] },
                  "state": { "queued": {} },
                  "isEnabled": true,
                  "isResumable": true,
                  "position": 1
                }
              ]
            }
          ]
        }
        "#;
    std::fs::write(&path, json).unwrap();

    let loaded = JsonStateStore::new(&path).load().await;
    let items = &loaded.packages[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].is_resumable(), Some(false));
    assert_eq!(items[1].is_resumable(), Some(true));
    // The rest of the item must survive too, not just the flag under test.
    assert_eq!(items[0].completed().ranges(), [ByteRange::new(0, 10)]);
    assert_eq!(items[0].total_bytes(), Some(100));
}

/// Case 2: the key omitted entirely — what the encoder writes for an
/// unprobed item. It must decode as `None`, not fail the whole load.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_omitting_is_resumable_decodes_as_not_yet_probed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let json = r#"
        {
          "formatVersion": 1,
          "packages": [
            {
              "id": "8B687F15-C739-421D-9E0F-1D2EFD8AC989",
              "name": "Season 1",
              "position": 0,
              "priority": 2,
              "items": [
                {
                  "id": "37B1C7A5-EDCF-4DA5-8849-FED5AC7AF237",
                  "url": "https://example.com/a.bin",
                  "filename": "a.bin",
                  "completed": { "ranges": [] },
                  "state": { "queued": {} },
                  "isEnabled": true,
                  "position": 0
                }
              ]
            }
          ]
        }
        "#;
    std::fs::write(&path, json).unwrap();

    let loaded = JsonStateStore::new(&path).load().await;
    let item = &loaded.packages[0].items[0];
    assert_eq!(item.is_resumable(), None);
    assert_eq!(item.output_filename, "a.bin");
}

// MARK: - Swift-written fixtures

/// Loads `name` through `JsonStateStore`, saves it back, flushes, and returns
/// the file Rust wrote as parsed JSON.
async fn load_save_flush(name: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("swift.json");
    std::fs::write(&source, fixture(name)).unwrap();

    let loaded = JsonStateStore::new(&source).load().await;
    assert!(!loaded.packages.is_empty(), "{name} did not load");

    let out = dir.path().join("out").join("state.json");
    let store = JsonStateStore::new(&out);
    store.save(loaded).await;
    store.flush().await;
    serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loads_state_written_by_swift() {
    for name in ["state_rich.json", "state_minimal.json"] {
        let written = load_save_flush(name).await;
        let expected: Value = serde_json::from_str(&fixture(name)).unwrap();
        assert_eq!(written, expected, "{name}");
    }

    // The legacy flat shape (format version 1) is rewritten as version 2 in
    // the nested shape. Components get a fresh id on decode: check it is a
    // well-formed uppercase UUID, then drop it before comparing.
    let mut written = load_save_flush("state_legacy_flat.json").await;
    for item in written["packages"][0]["items"].as_array_mut().unwrap() {
        let component = &mut item["components"][0];
        let id = component["id"].as_str().unwrap().to_owned();
        assert_eq!(id, id.to_uppercase());
        assert!(uuid_ok(&id));
        component.as_object_mut().unwrap().remove("id");
    }
    let expected = json!({
        "formatVersion": 2,
        "packages": [{
            "id": "00000000-0000-4000-8000-000000000020",
            "name": "Legacy",
            "position": 0,
            "priority": 2,
            "items": [
                {
                    "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
                    "components": [{
                        "url": "https://x/f.bin",
                        "partFilename": "f.bin",
                        "totalBytes": 2048,
                        "completed": {"ranges": [{"start": 0, "end": 1024}]},
                        "validator": "\"legacy-etag\"",
                        "origin": {"kind": "http"},
                        "isResumable": true
                    }],
                    "outputFilename": "f.bin",
                    "sourceURL": "https://x/f.bin",
                    "assembly": "none",
                    "state": {"stopped": {}},
                    "isEnabled": true,
                    "priority": 3,
                    "position": 3
                },
                {
                    "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA428",
                    "components": [{
                        "url": "https://x/g.bin",
                        "partFilename": "g.bin",
                        "completed": {"ranges": []},
                        "origin": {"kind": "http"}
                    }],
                    "outputFilename": "g.bin",
                    "sourceURL": "https://x/g.bin",
                    "assembly": "none",
                    "state": {"failed": {"reason": "boom"}},
                    "isEnabled": false,
                    "position": 0
                }
            ]
        }]
    });
    assert_eq!(written, expected);
}

fn uuid_ok(id: &str) -> bool {
    id.len() == 36 && Path::new(id).components().count() == 1 && id.matches('-').count() == 4
}
