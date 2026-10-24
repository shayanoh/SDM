mod support;

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use sdm_app::grabber::GrabberController;
use sdm_app::protocol::{AutoClearInterval, SettingsDto};
use sdm_grabber::{Clock, FakeProbeOrigin};

struct Kit {
    harness: support::Harness,
    controller: Arc<GrabberController>,
    settings: Arc<RwLock<SettingsDto>>,
    now: Arc<Mutex<SystemTime>>,
}

async fn kit() -> Kit {
    let settings = Arc::new(RwLock::new(SettingsDto::default()));
    let now = Arc::new(Mutex::new(SystemTime::UNIX_EPOCH));
    let clock_now = now.clone();
    let clock: Clock = Arc::new(move || *clock_now.lock().unwrap());
    let controller = GrabberController::new_with_settings(
        Arc::new(FakeProbeOrigin::new()),
        None,
        Arc::new(std::sync::atomic::AtomicBool::new(true)),
        Some(clock),
        settings.clone(),
    );
    let harness = support::harness_with(
        sdm_engine::FakeOrigin::new(support::payload(10)),
        Some(controller.clone()),
    )
    .await;
    Kit {
        harness,
        controller,
        settings,
        now,
    }
}

async fn rows(kit: &Kit) -> usize {
    kit.harness.app.tick().await.unwrap();
    support::grabber_view(&kit.harness).rows.len()
}

async fn ingest_one(kit: &Kit) {
    kit.harness
        .app
        .grabber_ingest_text("https://a.example.com/x.zip")
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quality_preferences_follow_the_settings() {
    let kit = kit().await;
    assert_eq!(kit.controller.quality_preferences().max_height, 1080);
    kit.settings.write().unwrap().yt_max_height = 480;
    kit.settings.write().unwrap().allow_av1 = false;
    let preferences = kit.controller.quality_preferences();
    assert_eq!(preferences.max_height, 480);
    assert!(
        !preferences
            .video_codecs
            .contains(&sdm_core::VideoCodec::Av1)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prune_uses_the_stored_interval() {
    let kit = kit().await;
    ingest_one(&kit).await;
    assert_eq!(rows(&kit).await, 1);

    // Default is 30 minutes: 20 minutes in, the row stays.
    *kit.now.lock().unwrap() = SystemTime::UNIX_EPOCH + Duration::from_secs(20 * 60);
    kit.controller.prune();
    assert_eq!(rows(&kit).await, 1);

    // At 15 minutes the same row is already old enough to go.
    kit.settings.write().unwrap().auto_clear_interval = AutoClearInterval::FifteenMinutes;
    kit.controller.prune();
    assert_eq!(rows(&kit).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn never_keeps_rows_forever() {
    let kit = kit().await;
    kit.settings.write().unwrap().auto_clear_interval = AutoClearInterval::Never;
    ingest_one(&kit).await;
    *kit.now.lock().unwrap() = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * 24 * 3600);
    kit.controller.prune();
    assert_eq!(rows(&kit).await, 1);
}
