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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_setters_succeed_and_reject_an_empty_binary_name() {
    within(async {
        let dir = scratch_directory();
        let (engine, _trash) = engine_over(payload(16), dir.path());
        engine
            .set_cookie_source(sdm_resolve::CookieSource::Firefox)
            .unwrap();
        engine.set_max_playlist_videos(25).unwrap();
        engine
            .set_yt_dlp_extra_arguments(vec!["--no-warnings".to_owned()])
            .unwrap();
        engine
            .set_binary_override("yt-dlp".to_owned(), Some(dir.path().join("yt-dlp")))
            .unwrap();
        engine
            .set_binary_override("yt-dlp".to_owned(), None)
            .unwrap();
    })
    .await;
}
