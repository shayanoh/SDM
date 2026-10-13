mod support;

use std::collections::HashSet;
use std::sync::Arc;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, Priority};
use sdm_engine::{EngineSettings, EngineSnapshot, JsonStateStore};
use sdm_ffi::{CoreError, DownloadEngine, EngineConfig};
use sdm_resolve::CookieSource;
use support::{
    ScratchTrash, dependencies, engine_over, engine_settings, package, payload, scratch_directory,
    su, wait_until, within,
};
use uuid::Uuid;

fn ids_of(snapshot: &EngineSnapshot, package_name: &str) -> Vec<Uuid> {
    snapshot
        .packages
        .iter()
        .find(|p| p.name == package_name)
        .map(|p| p.items.iter().map(|i| i.id).collect())
        .unwrap_or_default()
}

fn package_id(snapshot: &EngineSnapshot, package_name: &str) -> Uuid {
    snapshot
        .packages
        .iter()
        .find(|p| p.name == package_name)
        .unwrap_or_else(|| panic!("no package named {package_name}"))
        .id
}

fn two_item_package(name: &str, files: [&str; 2]) -> DownloadPackage {
    let items = files
        .iter()
        .map(|file| {
            DownloadItem::http(su(&format!("https://example.com/{file}")), *file)
                .with_enabled(false)
        })
        .collect();
    DownloadPackage::new(name).with_items(items)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_builds_a_production_engine_over_a_temp_state_file() {
    within(async {
        let dir = scratch_directory();
        let engine = DownloadEngine::new(EngineConfig {
            state_file: dir.path().join("state.json"),
            settings: engine_settings(dir.path()),
            binary_search_paths: vec![dir.path().join("bin")],
            cookie_source: CookieSource::None,
            max_playlist_videos: 50,
            yt_dlp_extra_arguments: vec![],
        })
        .expect("build the production engine");

        let snapshot = engine.snapshot().await.unwrap();
        assert!(snapshot.packages.is_empty());
        engine.restore().await.unwrap();
        engine.tick().await.unwrap();
        engine.shutdown().await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_exported_method_accepts_valid_arguments() {
    within(async {
        let dir = scratch_directory();
        let (engine, _trash) = engine_over(payload(4096), dir.path());

        // add
        engine
            .add(two_item_package("Alpha", ["a1.bin", "a2.bin"]))
            .await
            .unwrap();
        engine
            .add(two_item_package("Beta", ["b1.bin", "b2.bin"]))
            .await
            .unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(snapshot.packages.len(), 2);
        let alpha = package_id(&snapshot, "Alpha");
        let beta = package_id(&snapshot, "Beta");
        let alpha_ids = ids_of(&snapshot, "Alpha");
        let beta_ids = ids_of(&snapshot, "Beta");
        let (a1, a2) = (alpha_ids[0], alpha_ids[1]);
        let (b1, b2) = (beta_ids[0], beta_ids[1]);

        // set_enabled: the Disable/Enable axis shows in the snapshot.
        engine.set_enabled(true, a1).await.unwrap();
        engine.set_enabled(false, a1).await.unwrap();
        engine.set_enabled(true, a2).await.unwrap();
        engine.set_enabled(false, a2).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert!(snapshot.packages[0].items.iter().all(|i| !i.is_enabled));

        // set_priority, set_segment_count, retry, retry_mux, update_settings
        engine.set_priority(Some(Priority::High), a1).await.unwrap();
        engine.set_priority(None, a1).await.unwrap();
        engine.set_segment_count(3, a1).await.unwrap();
        engine.retry(a1).await.unwrap();
        engine.retry_mux(a1).await.unwrap();
        engine
            .update_settings(EngineSettings::new(3, 4, 16, dir.path().to_path_buf()))
            .await
            .unwrap();
        engine.pause_all().await.unwrap();
        engine.resume_all().await.unwrap();

        // tick, snapshot, speed_histories
        engine.tick().await.unwrap();
        let histories = engine
            .speed_histories(HashSet::from([a1, Uuid::new_v4()]))
            .await
            .unwrap();
        assert!(histories.len() <= 1, "an unknown item has no history");

        // reorder_items
        engine.reorder_items(vec![a2, a1], alpha).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Alpha"), vec![a2, a1]);

        // move_item (into Alpha at the front), move_items_to_package
        engine.move_item(b1, alpha, Some(0)).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Alpha"), vec![b1, a2, a1]);
        engine
            .move_items_to_package(vec![a1], beta, None)
            .await
            .unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Beta"), vec![b2, a1]);

        // move_items_to_new_package
        engine
            .move_items_to_new_package(vec![a2], "Gamma".to_owned())
            .await
            .unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Gamma"), vec![a2]);
        assert_eq!(snapshot.packages.len(), 3);

        // reorder_packages
        let gamma = package_id(&snapshot, "Gamma");
        engine
            .reorder_packages(vec![gamma, beta, alpha])
            .await
            .unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        let names: Vec<&str> = snapshot.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Gamma", "Beta", "Alpha"]);

        // reset_download, remove_item, remove_package
        engine.reset_download(a1).await.unwrap();
        engine.remove_item(a1, false).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Beta"), vec![b2]);
        engine.remove_package(gamma, false).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert!(snapshot.packages.iter().all(|p| p.name != "Gamma"));

        // start_item / stop_item on an enabled item of its own package.
        engine.add(package("Solo", "solo.bin", true)).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        let solo = ids_of(&snapshot, "Solo")[0];
        engine.start_item(solo).await.unwrap();
        engine.stop_item(solo).await.unwrap();

        // restore is idempotent, shutdown too.
        engine.restore().await.unwrap();
        engine.shutdown().await.unwrap();
        engine.shutdown().await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_exported_method_ignores_an_unknown_id() {
    within(async {
        let dir = scratch_directory();
        let (engine, trash) = engine_over(payload(4096), dir.path());
        engine
            .add(two_item_package("Alpha", ["a1.bin", "a2.bin"]))
            .await
            .unwrap();
        let before = engine.snapshot().await.unwrap();
        let ghost = Uuid::new_v4();
        let ghost_package = Uuid::new_v4();

        engine.set_enabled(true, ghost).await.unwrap();
        engine.start_item(ghost).await.unwrap();
        engine.retry(ghost).await.unwrap();
        engine.retry_mux(ghost).await.unwrap();
        engine.stop_item(ghost).await.unwrap();
        engine
            .set_priority(Some(Priority::Low), ghost)
            .await
            .unwrap();
        engine.set_segment_count(2, ghost).await.unwrap();
        engine
            .reorder_items(vec![ghost], ghost_package)
            .await
            .unwrap();
        engine.move_item(ghost, ghost_package, None).await.unwrap();
        engine
            .move_items_to_package(vec![ghost], ghost_package, Some(7))
            .await
            .unwrap();
        engine.remove_item(ghost, true).await.unwrap();
        engine.remove_package(ghost_package, true).await.unwrap();
        engine.reset_download(ghost).await.unwrap();
        let histories = engine
            .speed_histories(HashSet::from([ghost]))
            .await
            .unwrap();
        assert!(histories.is_empty());
        engine.reorder_packages(vec![ghost_package]).await.unwrap();

        let after = engine.snapshot().await.unwrap();
        assert_eq!(after.packages, before.packages);
        assert!(trash.trashed().is_empty(), "nothing may reach the trash");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_arguments_are_errors_not_panics() {
    within(async {
        let dir = scratch_directory();
        let (engine, _trash) = engine_over(payload(16), dir.path());

        let mut bad = DownloadPackage::new("Nameless");
        bad.name.clear();
        assert!(matches!(
            engine.add(bad).await,
            Err(CoreError::InvalidArgument { .. })
        ));

        let mut empty_item = DownloadItem::http(su("https://example.com/x.bin"), "x.bin");
        empty_item.components.clear();
        let with_empty_item = DownloadPackage::new("P").with_items(vec![empty_item]);
        assert!(matches!(
            engine.add(with_empty_item).await,
            Err(CoreError::InvalidArgument { .. })
        ));

        let mut nameless_file = DownloadItem::http(su("https://example.com/x.bin"), "x.bin");
        nameless_file.output_filename.clear();
        assert!(matches!(
            engine
                .add(DownloadPackage::new("P").with_items(vec![nameless_file]))
                .await,
            Err(CoreError::InvalidArgument { .. })
        ));

        let mut zero = engine_settings(dir.path());
        zero.max_concurrent = 0;
        assert!(matches!(
            engine.update_settings(zero).await,
            Err(CoreError::InvalidArgument { .. })
        ));
        let mut negative = engine_settings(dir.path());
        negative.checkpoint_interval_bytes = 0;
        assert!(matches!(
            engine.update_settings(negative).await,
            Err(CoreError::InvalidArgument { .. })
        ));

        assert!(matches!(
            engine.set_segment_count(0, Uuid::new_v4()).await,
            Err(CoreError::InvalidArgument { .. })
        ));
        assert!(matches!(
            engine
                .move_items_to_new_package(vec![Uuid::new_v4()], String::new())
                .await,
            Err(CoreError::InvalidArgument { .. })
        ));
        assert!(matches!(
            engine.set_binary_override(String::new(), None),
            Err(CoreError::InvalidArgument { .. })
        ));

        // Construction refuses invalid settings instead of asserting.
        let mut zero = engine_settings(dir.path());
        zero.global_max_connections = 0;
        let refused = DownloadEngine::new(EngineConfig {
            state_file: dir.path().join("state.json"),
            settings: zero,
            binary_search_paths: vec![],
            cookie_source: CookieSource::None,
            max_playlist_videos: 50,
            yt_dlp_extra_arguments: vec![],
        });
        assert!(matches!(refused, Err(CoreError::InvalidArgument { .. })));

        // And the engine is still fine after all that.
        assert!(engine.snapshot().await.unwrap().packages.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_completes_through_the_ffi() {
    within(async {
        let dir = scratch_directory();
        let data = payload(256 * 1024);
        let (engine, _trash) = engine_over(data.clone(), dir.path());
        engine.add(package("Files", "big.bin", true)).await.unwrap();
        let id = ids_of(&engine.snapshot().await.unwrap(), "Files")[0];

        wait_until(|| {
            let engine = Arc::clone(&engine);
            async move {
                engine.tick().await.unwrap();
                let snapshot = engine.snapshot().await.unwrap();
                snapshot
                    .packages
                    .iter()
                    .flat_map(|p| &p.items)
                    .any(|item| item.id == id && item.state == ItemState::Completed)
            }
        })
        .await;

        let written = std::fs::read(dir.path().join("Files").join("big.bin"))
            .expect("the finished file is in the package folder");
        assert_eq!(written, data);
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(snapshot.figures.running_count, 0);
        assert_eq!(
            snapshot.packages[0].aggregate.completed_bytes,
            data.len() as i64
        );
        engine.shutdown().await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_survives_shutdown_and_restore() {
    within(async {
        let dir = scratch_directory();
        let state_file = dir.path().join("state.json");
        let build = || {
            DownloadEngine::with_dependencies(
                engine_settings(dir.path()),
                dependencies(
                    sdm_engine::FakeOrigin::new(payload(16)),
                    Arc::new(JsonStateStore::new(&state_file)),
                    Arc::new(ScratchTrash::new()),
                ),
                DownloadEngine::default_tools(),
            )
            .expect("build the engine")
        };

        let first = build();
        first
            .add(two_item_package("Kept", ["k1.bin", "k2.bin"]))
            .await
            .unwrap();
        let expected = ids_of(&first.snapshot().await.unwrap(), "Kept");
        first.shutdown().await.unwrap();
        assert!(state_file.exists(), "shutdown flushes the state file");

        let second = build();
        second.restore().await.unwrap();
        let snapshot = second.snapshot().await.unwrap();
        assert_eq!(ids_of(&snapshot, "Kept"), expected);
    })
    .await;
}

fn state_of(snapshot: &EngineSnapshot, id: Uuid) -> ItemState {
    item_of(snapshot, id).state.clone()
}

fn item_of(snapshot: &EngineSnapshot, id: Uuid) -> &sdm_engine::ItemSnapshot {
    snapshot
        .packages
        .iter()
        .flat_map(|p| &p.items)
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no item {id}"))
}

/// Waits until `id` is in `state` (the engine moves items from spawned tasks,
/// so a state is observed rather than assumed).
async fn expect_state(engine: &Arc<DownloadEngine>, id: Uuid, state: ItemState) {
    let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state_of(&engine.snapshot().await.unwrap(), id) == state {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if reached.is_err() {
        let snapshot = engine.snapshot().await.unwrap();
        panic!(
            "expected {state:?}, the item is {:?}",
            state_of(&snapshot, id)
        );
    }
}

/// Each lifecycle call changes what the next snapshot shows. The transport is
/// held at a gate so an item stays `Running` for as long as the test needs,
/// and with one slot the scheduler's choices are visible in the states.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_calls_change_what_the_snapshot_shows() {
    within(async {
        let dir = scratch_directory();
        let gate = support::GateTransport::new(sdm_engine::FakeOrigin::new(payload(4096)));
        let (engine, _trash) = support::engine_with(
            gate.clone(),
            EngineSettings::new(1, 2, 8, dir.path().to_path_buf()),
        );
        let items: Vec<DownloadItem> = ["x.bin", "y.bin", "z.bin"]
            .iter()
            .map(|file| {
                DownloadItem::http(su(&format!("https://example.com/{file}")), *file)
                    .with_state(ItemState::Stopped)
            })
            .collect();
        engine
            .add(DownloadPackage::new("Held").with_items(items))
            .await
            .unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        let ids = ids_of(&snapshot, "Held");
        let (x, y, z) = (ids[0], ids[1], ids[2]);
        for id in [x, y, z] {
            assert_eq!(state_of(&snapshot, id), ItemState::Stopped);
        }

        // start_item: x takes the one slot, y and z wait behind it.
        engine.start_item(x).await.unwrap();
        expect_state(&engine, x, ItemState::Running).await;
        engine.start_item(y).await.unwrap();
        engine.start_item(z).await.unwrap();
        expect_state(&engine, y, ItemState::Queued).await;
        expect_state(&engine, z, ItemState::Queued).await;

        // set_priority: z, last in line, is chosen first once x lets go.
        engine
            .set_priority(Some(Priority::Highest), z)
            .await
            .unwrap();
        // stop_item: x stops (it stays enabled) and its slot goes to z.
        engine.stop_item(x).await.unwrap();
        expect_state(&engine, x, ItemState::Stopped).await;
        expect_state(&engine, z, ItemState::Running).await;
        expect_state(&engine, y, ItemState::Queued).await;
        assert!(item_of(&engine.snapshot().await.unwrap(), x).is_enabled);

        // set_segment_count shows as the item's configured segments.
        engine.set_segment_count(5, z).await.unwrap();
        wait_until(|| {
            let engine = Arc::clone(&engine);
            async move { item_of(&engine.snapshot().await.unwrap(), z).configured_segments == 5 }
        })
        .await;

        // pause_all stops everything, resume_all starts everything again.
        engine.pause_all().await.unwrap();
        for id in [x, y, z] {
            expect_state(&engine, id, ItemState::Stopped).await;
        }
        engine.resume_all().await.unwrap();
        wait_until(|| {
            let engine = Arc::clone(&engine);
            async move {
                let snapshot = engine.snapshot().await.unwrap();
                let running = [x, y, z]
                    .iter()
                    .filter(|id| state_of(&snapshot, **id) == ItemState::Running)
                    .count();
                let queued = [x, y, z]
                    .iter()
                    .filter(|id| state_of(&snapshot, **id) == ItemState::Queued)
                    .count();
                running == 1 && queued == 2
            }
        })
        .await;

        // update_settings: three slots start the two that were waiting.
        engine
            .update_settings(EngineSettings::new(3, 2, 8, dir.path().to_path_buf()))
            .await
            .unwrap();
        for id in [x, y, z] {
            expect_state(&engine, id, ItemState::Running).await;
        }

        // Let them finish, then reset_download sends one back to the start.
        gate.set_open(true);
        for id in [x, y, z] {
            expect_state(&engine, id, ItemState::Completed).await;
        }
        gate.set_open(false);
        engine.reset_download(x).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_ne!(state_of(&snapshot, x), ItemState::Completed);
        assert_eq!(item_of(&snapshot, x).completed.total_bytes(), 0);
        assert_eq!(state_of(&snapshot, y), ItemState::Completed);
        engine.shutdown().await.unwrap();
    })
    .await;
}

/// `retry` takes a `Failed` item back into the queue; `retry_mux` leaves an
/// item that is not a failed mux alone. (A failed mux needs ffmpeg and every
/// component complete, which these tests cannot stage; the call's reach into
/// the engine is covered by the unknown-id test and the engine's own tests.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_requeues_a_failed_item() {
    within(async {
        let dir = scratch_directory();
        let origin = sdm_engine::FakeOrigin::with_behavior(
            payload(4096),
            sdm_engine::FakeOriginBehavior {
                status_override: Some(404),
                ..Default::default()
            },
        );
        let gate = support::GateTransport::new(origin.clone());
        gate.set_open(true);
        let (engine, _trash) = support::engine_with(
            gate.clone(),
            EngineSettings::new(1, 1, 8, dir.path().to_path_buf()),
        );
        engine.add(package("Broken", "z.bin", true)).await.unwrap();
        let id = ids_of(&engine.snapshot().await.unwrap(), "Broken")[0];
        let failed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                engine.tick().await.unwrap();
                if matches!(
                    state_of(&engine.snapshot().await.unwrap(), id),
                    ItemState::Failed { .. }
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            failed.is_ok(),
            "the item is {:?}",
            state_of(&engine.snapshot().await.unwrap(), id)
        );

        engine.retry_mux(id).await.unwrap();
        assert!(matches!(
            state_of(&engine.snapshot().await.unwrap(), id),
            ItemState::Failed { .. }
        ));

        gate.set_open(false);
        origin.set_behavior(sdm_engine::FakeOriginBehavior::default());
        engine.retry(id).await.unwrap();
        expect_state(&engine, id, ItemState::Running).await;
        engine.shutdown().await.unwrap();
    })
    .await;
}

/// The setters change what the next yt-dlp run is given: the cookie source,
/// the extra arguments and the playlist cap reach the command line (and the
/// binary override picks the executable). The engine's resolver is the one
/// built from the same toolchain the setters write to, run over a recording
/// process runner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_setters_reach_the_yt_dlp_command_line() {
    use sdm_core::ResolvedTarget;
    use sdm_resolve::testing::{FakeProcessRunner, ok};
    use std::os::unix::fs::PermissionsExt;

    within(async {
        let dir = scratch_directory();
        let entries: Vec<String> = (0..30)
            .map(|i| {
                format!(
                    r#"{{"id":"v{i}","url":"https://www.youtube.com/watch?v=v{i}","title":"T{i}"}}"#
                )
            })
            .collect();
        let playlist = format!(
            r#"{{"_type":"playlist","title":"P","entries":[{}]}}"#,
            entries.join(",")
        );
        let mut runner = FakeProcessRunner::new();
        runner.responses = vec![("--flat-playlist".into(), ok(playlist))];
        let runner = Arc::new(runner);
        let tools = sdm_ffi::shared::Toolchain::with_runner(vec![], runner.clone());
        let resolver = tools.resolver();
        let mut deps = dependencies(
            sdm_engine::FakeOrigin::new(payload(16)),
            Arc::new(sdm_engine::InMemoryStateStore::new()),
            Arc::new(ScratchTrash::new()),
        );
        deps.resolver = Some(resolver.clone());
        let engine =
            DownloadEngine::with_dependencies(engine_settings(dir.path()), deps, tools).unwrap();

        let fake_binary = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let first = fake_binary("yt-dlp-first");
        let second = fake_binary("yt-dlp-second");
        let playlist_url = su("https://www.youtube.com/playlist?list=PLx");

        engine
            .set_binary_override("yt-dlp".to_owned(), Some(first.clone()))
            .unwrap();
        engine.set_cookie_source(CookieSource::Firefox).unwrap();
        engine.set_max_playlist_videos(10).unwrap();
        engine
            .set_yt_dlp_extra_arguments(vec!["--sentinel-extra".to_owned()])
            .unwrap();
        let target = resolver.resolve(&playlist_url).await;
        let Ok(ResolvedTarget::Playlist {
            entries,
            total_available,
            ..
        }) = target
        else {
            panic!("expected a playlist, got {target:?}");
        };
        assert_eq!(entries.len(), 10, "the playlist cap reaches the resolver");
        assert_eq!(total_available, 30);
        let call = runner.calls().pop().expect("yt-dlp was run");
        assert_eq!(call.executable, first, "the override picks the executable");
        let args = call.arguments.join(" ");
        assert!(args.contains("--cookies-from-browser firefox"), "{args}");
        assert!(args.ends_with("--sentinel-extra"), "{args}");

        // Changing each setting again changes the next run.
        engine
            .set_binary_override("yt-dlp".to_owned(), Some(second.clone()))
            .unwrap();
        engine.set_cookie_source(CookieSource::None).unwrap();
        engine.set_max_playlist_videos(25).unwrap();
        engine.set_yt_dlp_extra_arguments(vec![]).unwrap();
        let Ok(ResolvedTarget::Playlist { entries, .. }) = resolver.resolve(&playlist_url).await
        else {
            panic!("expected a playlist");
        };
        assert_eq!(entries.len(), 25);
        let call = runner.calls().pop().unwrap();
        assert_eq!(call.executable, second);
        let args = call.arguments.join(" ");
        assert!(!args.contains("--cookies-from-browser"), "{args}");
        assert!(!args.contains("--sentinel-extra"), "{args}");
    })
    .await;
}
