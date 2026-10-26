mod support;

use std::sync::Arc;

use sdm_app::host::{FakeHost, RecordingSink};
use sdm_app::notify::Notification;
use sdm_app::protocol::{ItemStateDto, SettingsDto};
use sdm_app::{App, Dependencies};
use sdm_engine::{FakeOrigin, InMemoryStateStore, RetryPolicy, Trash};
use support::*;

fn finished_notifications(h: &Harness) -> usize {
    h.host
        .notifications()
        .iter()
        .filter(|n| matches!(n, Notification::Finished { .. }))
        .count()
}

async fn complete_one_download(h: &Harness) {
    h.app
        .add_url("https://example.com/files/data.bin", None)
        .await
        .unwrap();
    let done = tick_until(&h.app, 400, || {
        h.sink
            .patches()
            .iter()
            .filter_map(|p| p.structure.as_ref())
            .flat_map(|s| s.packages.iter())
            .flat_map(|p| p.items.iter())
            .any(|i| i.state == ItemStateDto::Completed)
    })
    .await;
    assert!(done, "the item never completed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_download_notifies_once_and_a_later_tick_adds_none() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    complete_one_download(&h).await;
    assert_eq!(finished_notifications(&h), 1);
    h.app.tick().await.unwrap();
    h.app.tick().await.unwrap();
    assert_eq!(finished_notifications(&h), 1);
    assert!(
        !h.host
            .notifications()
            .iter()
            .any(|n| matches!(n, Notification::PackageFinished { .. })),
        "a one-item package is announced by its item alone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_switched_off_toggle_silences_only_its_notification() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    h.app
        .update_settings(SettingsDto {
            notify_download_finished: false,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    complete_one_download(&h).await;
    assert_eq!(finished_notifications(&h), 0);
    assert!(h.host.notifications().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grabbing_links_notifies_once_per_new_row_and_a_recheck_adds_none() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.origin
        .set_behavior(online(), &su("https://a.example.com/one.mp4"));
    kit.origin
        .set_behavior(online(), &su("https://a.example.com/two.mp4"));
    let h = &kit.harness;
    h.app
        .grabber_ingest_text("https://a.example.com/one.mp4 https://a.example.com/two.mp4")
        .await
        .unwrap();
    h.app.tick().await.unwrap();
    let grabbed: Vec<_> = h
        .host
        .notifications()
        .into_iter()
        .filter(|n| matches!(n, Notification::LinksGrabbed { .. }))
        .collect();
    assert_eq!(grabbed, vec![Notification::LinksGrabbed { count: 2 }]);

    h.app.grabber_recheck_failed().await.unwrap();
    h.app.tick().await.unwrap();
    let again = h
        .host
        .notifications()
        .into_iter()
        .filter(|n| matches!(n, Notification::LinksGrabbed { .. }))
        .count();
    assert_eq!(again, 1, "the same rows are never announced twice");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_links_grabbed_toggle_silences_it() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.origin
        .set_behavior(online(), &su("https://a.example.com/one.mp4"));
    let h = &kit.harness;
    h.app
        .update_settings(SettingsDto {
            notify_links_grabbed: false,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    h.app
        .grabber_ingest_text("https://a.example.com/one.mp4")
        .await
        .unwrap();
    h.app.tick().await.unwrap();
    assert!(
        !h.host
            .notifications()
            .iter()
            .any(|n| matches!(n, Notification::LinksGrabbed { .. }))
    );
}

/// Never trashes anything; a launch test must not delete files.
struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _path: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::other("a launch test must not trash files"))
    }
}

fn app_over(host: &Arc<FakeHost>, store: &Arc<InMemoryStateStore>) -> Arc<App> {
    App::new(
        host.clone(),
        Dependencies {
            transport: Arc::new(FakeOrigin::new(payload(3000))),
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

/// The window attaches while the saved downloads are still loading, so the
/// first snapshot it sees is empty; what `restore` then brings in is old news.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_finished_in_an_earlier_run_do_not_notify_on_launch() {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryStateStore::new());

    let first_host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let first = app_over(&first_host, &store);
    let sink = Arc::new(RecordingSink::default());
    first.attach(sink.clone()).await.unwrap();
    first
        .add_url("https://example.com/files/data.bin", None)
        .await
        .unwrap();
    let done = tick_until(&first, 400, || {
        sink.patches()
            .iter()
            .filter_map(|p| p.structure.as_ref())
            .flat_map(|s| s.packages.iter())
            .flat_map(|p| p.items.iter())
            .any(|i| i.state == ItemStateDto::Completed)
    })
    .await;
    assert!(done, "the item never completed");
    first.shutdown().await;

    let host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let second = app_over(&host, &store);
    second
        .attach(Arc::new(RecordingSink::default()))
        .await
        .unwrap();
    second.restore().await.unwrap();
    for _ in 0..3 {
        second.tick().await.unwrap();
    }
    assert!(
        host.notifications().is_empty(),
        "{:?}",
        host.notifications()
    );
}
