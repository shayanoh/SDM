mod support;

use sdm_app::protocol::{GrabberRow, SettingsDto};
use sdm_engine::FakeOrigin;
use support::*;

fn row_count(h: &Harness) -> usize {
    grabber_view(h).rows.len()
}

async fn kit(watching: bool) -> GrabberKit {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness
        .app
        .update_settings(SettingsDto {
            clipboard_watching_enabled: watching,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
    for url in [
        "https://a.example.com/one.mp4",
        "https://a.example.com/two.mp4",
    ] {
        kit.origin.set_behavior(online(), &su(url));
    }
    kit
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_link_in_one_copy_is_grabbed() {
    let kit = kit(true).await;
    kit.harness
        .app
        .clipboard_changed(Some(
            "look: https://a.example.com/one.mp4 and\nhttps://a.example.com/two.mp4.".into(),
        ))
        .await
        .unwrap();
    assert_eq!(row_count(&kit.harness), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prose_without_a_link_grabs_nothing() {
    let kit = kit(true).await;
    kit.harness.sink.take();
    kit.harness
        .app
        .clipboard_changed(Some("a recipe for soup".into()))
        .await
        .unwrap();
    kit.harness.app.clipboard_changed(None).await.unwrap();
    assert!(kit.harness.sink.take().iter().all(|p| p.grabber.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_copy_twice_grabs_once() {
    let kit = kit(true).await;
    let text = "https://a.example.com/one.mp4".to_owned();
    kit.harness
        .app
        .clipboard_changed(Some(text.clone()))
        .await
        .unwrap();
    kit.harness.app.clipboard_changed(Some(text)).await.unwrap();
    assert_eq!(row_count(&kit.harness), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_is_grabbed_while_watching_is_off() {
    let kit = kit(false).await;
    assert!(!kit.harness.app.clipboard_watching_enabled());
    kit.harness.sink.take();
    kit.harness
        .app
        .clipboard_changed(Some("https://a.example.com/one.mp4".into()))
        .await
        .unwrap();
    assert!(kit.harness.sink.take().iter().all(|p| p.grabber.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_sdm_copied_itself_is_not_grabbed_again() {
    let kit = kit(true).await;
    let h = &kit.harness;
    // The earlier copy has prose around the link, so the bare address SDM
    // copies later is different text from anything seen before.
    h.app
        .clipboard_changed(Some("see https://a.example.com/two.mp4 now".into()))
        .await
        .unwrap();
    let id = link_id(h);
    let url = h.app.grabber_copy_url(&id).unwrap();
    h.app.grabber_remove_links(&[id]).await.unwrap();
    assert_eq!(row_count(h), 0);
    h.app.clipboard_changed(Some(url)).await.unwrap();
    assert_eq!(row_count(h), 0, "our own copy came back as a new grab");
}

fn link_id(h: &Harness) -> String {
    grabber_view(h)
        .rows
        .iter()
        .find_map(|r| match r {
            GrabberRow::Link(l) => Some(l.id.clone()),
            _ => None,
        })
        .expect("a link row")
}
