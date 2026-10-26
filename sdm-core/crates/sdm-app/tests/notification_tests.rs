mod support;

use sdm_app::notify::Notification;
use sdm_app::protocol::{ItemStateDto, SettingsDto};
use sdm_engine::FakeOrigin;
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
        h.host
            .notifications()
            .iter()
            .any(|n| matches!(n, Notification::PackageFinished { .. })),
        "the one-item package finished too"
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
    assert!(
        h.host
            .notifications()
            .iter()
            .any(|n| matches!(n, Notification::PackageFinished { .. })),
        "the package toggle is still on"
    );
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
