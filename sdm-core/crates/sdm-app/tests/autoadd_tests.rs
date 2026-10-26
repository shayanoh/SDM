mod support;

use sdm_app::protocol::SettingsDto;
use sdm_engine::FakeOrigin;
use sdm_grabber::FakeProbeOriginBehavior;
use support::*;

fn engine_item_count(h: &Harness) -> usize {
    h.sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages.iter().map(|p| p.items.len()).sum())
        .unwrap_or(0)
}

async fn kit(auto_add: bool, urls: &[(&str, FakeProbeOriginBehavior)]) -> GrabberKit {
    let kit = grabber_kit(Some(standard_resolver()), FakeOrigin::new(payload(1000))).await;
    kit.harness
        .app
        .update_settings(SettingsDto {
            auto_add_and_start_on_grab: auto_add,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    for (url, behavior) in urls {
        kit.origin.set_behavior(behavior.clone(), &su(url));
    }
    let text = urls.iter().map(|(u, _)| *u).collect::<Vec<_>>().join(" ");
    kit.harness.app.grabber_ingest_text(&text).await.unwrap();
    kit
}

fn offline() -> FakeProbeOriginBehavior {
    FakeProbeOriginBehavior {
        status_code: 404,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_is_added_while_the_setting_is_off() {
    let kit = kit(false, &[("https://a.example.com/good.zip", online())]).await;
    for _ in 0..5 {
        kit.harness.app.tick().await.unwrap();
    }
    assert_eq!(engine_item_count(&kit.harness), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ready_package_is_added_and_started_once() {
    let kit = kit(true, &[("https://a.example.com/good.zip", online())]).await;
    let h = &kit.harness;
    let added = tick_until(&h.app, 50, || engine_item_count(h) == 1).await;
    assert!(added, "the ready package was never added");
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    assert_eq!(engine_item_count(h), 1, "it is not added a second time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_offline_link_is_not_added() {
    let kit = kit(true, &[("https://a.example.com/gone.zip", offline())]).await;
    for _ in 0..5 {
        kit.harness.app.tick().await.unwrap();
    }
    assert_eq!(engine_item_count(&kit.harness), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_media_row_that_waits_for_a_format_choice_is_not_added() {
    // `youtu.be/big…` only offers 2160p, so nothing matches the preferences.
    let kit = kit(true, &[("https://youtu.be/big1", online())]).await;
    for _ in 0..5 {
        kit.harness.app.tick().await.unwrap();
    }
    assert_eq!(engine_item_count(&kit.harness), 0);
}
