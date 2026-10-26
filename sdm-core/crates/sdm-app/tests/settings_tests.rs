mod support;

use std::sync::Arc;

use sdm_app::host::{FakeHost, RecordingSink};
use sdm_app::protocol::{ItemStateDto, Patch, SettingsDto};
use sdm_app::{App, Dependencies};
use sdm_engine::{FakeOrigin, FakeOriginBehavior, InMemoryStateStore, RetryPolicy, Trash};
use support::*;

/// These tests never delete a file; if one tried, it fails loudly.
struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _path: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "a settings test must not trash files",
        ))
    }
}

/// A body big enough, and a chunk small enough, that a download is still
/// running after a handful of ticks.
fn slow_origin() -> FakeOrigin {
    FakeOrigin::with_behavior(
        payload(40_000_000),
        FakeOriginBehavior {
            chunk_size: 16,
            ..FakeOriginBehavior::default()
        },
    )
}

fn stats_of(patches: &[Patch]) -> (u32, u32) {
    patches
        .iter()
        .rev()
        .find_map(|p| p.stats.as_ref())
        .map(|s| (s.running_count, s.queued_count))
        .expect("a patch with stats")
}

fn states(patches: &[Patch]) -> Vec<ItemStateDto> {
    patches
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| {
            s.packages
                .iter()
                .flat_map(|p| p.items.iter().map(|i| i.state.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn update_settings_changes_the_running_limit() {
    let h = harness(slow_origin()).await;
    for name in ["a", "b", "c"] {
        h.app
            .add_url(&format!("https://example.com/{name}.bin"), None)
            .await
            .unwrap();
    }
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    h.sink.take();
    h.app.full_state().await.unwrap();
    assert_eq!(stats_of(&h.sink.take()), (3, 0), "three at once by default");

    h.app
        .update_settings(SettingsDto {
            max_concurrent: 1,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    h.sink.take();
    h.app.full_state().await.unwrap();
    assert_eq!(stats_of(&h.sink.take()), (1, 2), "one running, two queued");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_survive_a_restart() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    h.app
        .update_settings(SettingsDto {
            max_concurrent: 7,
            theme_id: "deep-purple".into(),
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    let reloaded = sdm_app::settings::SettingsStore::load(
        h.data.path().join("settings.json"),
        h.downloads.path().to_path_buf(),
    )
    .get();
    assert_eq!(reloaded.max_concurrent, 7);
    assert_eq!(reloaded.theme_id, "deep-purple");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn update_settings_with_no_folder_leaves_the_folder() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let before = h.app.get_settings().download_folder;
    h.app
        .update_settings(SettingsDto {
            max_concurrent: 2,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    assert_eq!(h.app.get_settings().download_folder, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rejected_update_changes_nothing_in_the_engine_or_the_file() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let before = h.app.get_settings();
    let result = h
        .app
        .update_settings(SettingsDto {
            max_concurrent: 99,
            ..SettingsDto::default()
        })
        .await;
    assert!(result.is_err());
    assert_eq!(h.app.get_settings(), before);
}

/// An `App` over a state store shared between runs, so a second one can
/// restore what the first saved.
fn app_over(host: &Arc<FakeHost>, store: &Arc<InMemoryStateStore>, origin: FakeOrigin) -> Arc<App> {
    App::new(
        host.clone(),
        Dependencies {
            transport: Arc::new(origin),
            state_store: store.clone(),
            trash: Arc::new(NoTrash),
            resolver: None,
            muxer: None,
            wholesale_downloader: None,
            retry_policy: RetryPolicy::new(),
            binaries: None,
            grabber: None,
            settings: None,
        },
    )
    .expect("app")
}

async fn restart_with_auto_resume(auto_resume: bool, disable_the_item: bool) -> Vec<ItemStateDto> {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let store = Arc::new(InMemoryStateStore::new());

    let first = app_over(&host, &store, slow_origin());
    first
        .add_url("https://example.com/a.bin", None)
        .await
        .unwrap();
    let sink = Arc::new(RecordingSink::default());
    first.attach(sink.clone()).await.unwrap();
    first.tick().await.unwrap();
    let id = sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].items[0].id.clone())
        .expect("the item");
    first.stop_items(std::slice::from_ref(&id)).await.unwrap();
    if disable_the_item {
        first.set_items_enabled(&[id], false).await.unwrap();
    }
    first
        .update_settings(SettingsDto {
            auto_start_downloads_on_launch: auto_resume,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    first.shutdown().await;

    let second = app_over(&host, &store, slow_origin());
    let sink = Arc::new(RecordingSink::default());
    second.attach(sink.clone()).await.unwrap();
    second.restore().await.unwrap();
    for _ in 0..3 {
        second.tick().await.unwrap();
    }
    second.full_state().await.unwrap();
    states(&sink.patches())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_resume_on_launch_resumes_a_stopped_enabled_item() {
    let after = restart_with_auto_resume(true, false).await;
    assert!(
        matches!(
            after.as_slice(),
            [ItemStateDto::Running | ItemStateDto::Queued]
        ),
        "{after:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_auto_resume_everything_stays_stopped() {
    let after = restart_with_auto_resume(false, false).await;
    assert_eq!(after, vec![ItemStateDto::Stopped]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_resume_leaves_a_disabled_item_alone() {
    let after = restart_with_auto_resume(true, true).await;
    assert_eq!(after, vec![ItemStateDto::Stopped]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binaries_status_reports_bundled_from_the_host() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    // No binaries owner: nothing is bundled or downloading.
    let status = h.app.binaries_status();
    assert!(!status.bundled && !status.downloading);
    assert!(h.app.check_binaries_now().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_bundled_flag_follows_the_hosts_tool_folder() {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let host = Arc::new(
        FakeHost::new(data.path().to_path_buf(), downloads.path().to_path_buf())
            .with_bundled_tools_dir(data.path().join("vendor")),
    );
    let binaries = sdm_resolve::ManagedBinaries::new(
        data.path().join("bin"),
        Arc::new(sdm_resolve::testing::FakeBinaryFetcher::new()),
        Arc::new(sdm_resolve::testing::FakeProcessRunner::new()),
        Vec::new,
        || sdm_resolve::YtDlpChannel::Stable,
    );
    let store = Arc::new(InMemoryStateStore::new());
    let app = App::new(
        host,
        Dependencies {
            transport: Arc::new(FakeOrigin::new(payload(10))),
            state_store: store,
            trash: Arc::new(NoTrash),
            resolver: None,
            muxer: None,
            wholesale_downloader: None,
            retry_policy: RetryPolicy::new(),
            binaries: Some(binaries),
            grabber: None,
            settings: None,
        },
    )
    .unwrap();
    assert!(app.binaries_status().bundled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_themes_returns_the_bundled_themes_with_light_and_dark() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let themes = h.app.list_themes();
    let ids: Vec<&str> = themes.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"light") && ids.contains(&"dark"), "{ids:?}");
    assert!(ids.contains(&"deep-purple"), "{ids:?}");
    assert!(themes.iter().all(|t| !t.name.is_empty()));
}

#[test]
fn a_partial_settings_object_is_rejected() {
    let partial = serde_json::json!({ "launchAtLogin": true });
    assert!(
        serde_json::from_value::<SettingsDto>(partial).is_err(),
        "an omitted field must not silently fall back to its default"
    );
}

#[test]
fn a_full_object_without_download_folder_parses() {
    let mut full = serde_json::to_value(SettingsDto::default()).expect("encodes");
    full.as_object_mut()
        .expect("an object")
        .remove("downloadFolder");
    let parsed: SettingsDto = serde_json::from_value(full).expect("parses");
    assert_eq!(parsed.download_folder, None);
}
