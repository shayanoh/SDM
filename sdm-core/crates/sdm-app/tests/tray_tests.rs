mod support;

use sdm_engine::{FakeOrigin, FakeOriginBehavior};
use support::*;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_tray_status_follows_the_running_downloads_and_waiting_links() {
    let kit = grabber_kit(None, slow_origin()).await;
    let h = &kit.harness;
    assert_eq!(h.app.tray_status().running_count, 0);
    assert_eq!(
        h.app.tray_status().quit_warning(),
        None,
        "nothing running means nothing to warn about"
    );

    h.app
        .add_url("https://example.com/files/big.bin", None)
        .await
        .unwrap();
    let running = tick_until(&h.app, 50, || h.app.tray_status().running_count == 1).await;
    assert!(running, "the download never started");
    let status = h.app.tray_status();
    assert_eq!(status.items[0].name, "big.bin");
    assert!(status.quit_warning().is_some());

    kit.origin
        .set_behavior(online(), &su("https://a.example.com/one.mp4"));
    h.app
        .grabber_ingest_text("https://a.example.com/one.mp4")
        .await
        .unwrap();
    assert_eq!(h.app.tray_status().links_waiting, 1);
}
