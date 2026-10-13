mod support;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sdm_core::{FormatChoice, MediaContainer, QualityPreferences};
use sdm_ffi::grabber::validate_budget;
use sdm_ffi::shared::{GrabberSettings, Toolchain};
use sdm_ffi::{CoreError, GrabberConfig, GrabberSession};
use sdm_grabber::{
    FakeProbeOrigin, FakeProbeOriginBehavior, GrabberBudget, GrabberSnapshot, LinkProber, Stage,
    Verdict,
};
use sdm_resolve::CookieSource;
use support::{su, within};
use uuid::Uuid;

fn online() -> FakeProbeOriginBehavior {
    FakeProbeOriginBehavior {
        status_code: 200,
        headers: HashMap::from([
            ("content-type".to_owned(), "video/mp4".to_owned()),
            ("content-length".to_owned(), "5000000".to_owned()),
        ]),
        ..FakeProbeOriginBehavior::default()
    }
}

fn session_over(
    origin: &Arc<FakeProbeOrigin>,
    clock: Option<Arc<dyn Fn() -> SystemTime + Send + Sync>>,
) -> Arc<GrabberSession> {
    GrabberSession::with_dependencies(
        LinkProber::new(origin.clone(), false),
        GrabberBudget::default(),
        None,
        Arc::new(GrabberSettings::new(QualityPreferences::default(), true)),
        Toolchain::new(Vec::new()),
        clock,
    )
    .expect("build the session")
}

fn link_ids(snapshot: &GrabberSnapshot) -> Vec<Uuid> {
    snapshot.links.iter().map(|l| l.id).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_builds_a_production_session() {
    within(async {
        let session = GrabberSession::new(GrabberConfig {
            budget: GrabberBudget::default(),
            deep_sniff_enabled: true,
            binary_search_paths: vec![],
            cookie_source: CookieSource::None,
            max_playlist_videos: 50,
            yt_dlp_extra_arguments: vec![],
            quality_preferences: QualityPreferences::default(),
            ffmpeg_available: false,
        })
        .expect("build the production session");
        let snapshot = session.snapshot().unwrap();
        assert!(snapshot.links.is_empty());
        assert_eq!(snapshot.total_count, 0);
        // Text with no link in it ingests nothing and probes nothing.
        session
            .ingest_text("no links here".to_owned())
            .await
            .unwrap();
        assert_eq!(session.snapshot().unwrap().total_count, 0);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_exported_method_accepts_valid_arguments() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        origin.set_behavior(online(), &su("https://a.example.com/movie.mp4"));
        origin.set_behavior(online(), &su("https://b.example.com/show.mp4"));
        let session = session_over(&origin, None);

        // ingest_text, ingest_urls
        session
            .ingest_text("see https://a.example.com/movie.mp4 please".to_owned())
            .await
            .unwrap();
        session
            .ingest_urls(vec![su("https://b.example.com/show.mp4")])
            .await
            .unwrap();
        let snapshot = session.snapshot().unwrap();
        assert_eq!(snapshot.total_count, 2);
        assert!(snapshot.links.iter().all(|l| l.stage == Stage::Done));
        assert!(
            snapshot
                .links
                .iter()
                .all(|l| l.verdict == Some(Verdict::Online))
        );
        let ids = link_ids(&snapshot);

        // set_known_download_urls marks the matching link a duplicate.
        session
            .set_known_download_urls(HashSet::from([su("https://a.example.com/movie.mp4")]))
            .unwrap();
        let snapshot = session.snapshot().unwrap();
        assert!(
            snapshot
                .links
                .iter()
                .any(|l| l.is_duplicate && l.original_url == su("https://a.example.com/movie.mp4"))
        );

        // recheck, recheck_failed, cancel_checks
        session.recheck(HashSet::from([ids[0]])).await.unwrap();
        session.recheck_failed().await.unwrap();
        session.cancel_checks().unwrap();

        // Package edits.
        let packages = session.snapshot().unwrap().packages;
        assert!(!packages.is_empty());
        let first = packages[0].id;
        session.rename_package(first, "Renamed".to_owned()).unwrap();
        let snapshot = session.snapshot().unwrap();
        assert!(snapshot.packages.iter().any(|p| p.name == "Renamed"));
        session.split_package(first).unwrap();
        let snapshot = session.snapshot().unwrap();
        let (one, two) = (snapshot.packages[0].id, snapshot.packages[1].id);
        session.merge_packages(one, two).unwrap();
        let snapshot = session.snapshot().unwrap();
        assert_eq!(snapshot.packages.len(), 1);
        let target = snapshot.packages[0].id;
        session.move_link(ids[0], target).unwrap();
        session.remove_package(target).unwrap();
        assert_eq!(session.snapshot().unwrap().total_count, 0);

        // remove_link and clear.
        session
            .ingest_urls(vec![su("https://a.example.com/movie.mp4")])
            .await
            .unwrap();
        let again = link_ids(&session.snapshot().unwrap());
        session.remove_link(again[0]).unwrap();
        assert_eq!(session.snapshot().unwrap().total_count, 0);
        session
            .ingest_urls(vec![su("https://b.example.com/show.mp4")])
            .await
            .unwrap();
        session.clear().unwrap();
        assert_eq!(session.snapshot().unwrap().total_count, 0);

        // Settings setters.
        session.set_cookie_source(CookieSource::Chrome).unwrap();
        session.set_max_playlist_videos(10).unwrap();
        session
            .set_yt_dlp_extra_arguments(vec!["--no-warnings".to_owned()])
            .unwrap();
        session
            .set_binary_override("ffmpeg".to_owned(), None)
            .unwrap();
        session.set_ffmpeg_available(false).unwrap();
        session
            .set_quality_preferences(QualityPreferences::default())
            .unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_exported_method_ignores_an_unknown_id() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        origin.set_behavior(online(), &su("https://a.example.com/movie.mp4"));
        let session = session_over(&origin, None);
        session
            .ingest_urls(vec![su("https://a.example.com/movie.mp4")])
            .await
            .unwrap();
        let before = session.snapshot().unwrap();
        let ghost = Uuid::new_v4();
        let other_ghost = Uuid::new_v4();

        session.remove_link(ghost).unwrap();
        session.remove_package(ghost).unwrap();
        session.move_link(ghost, other_ghost).unwrap();
        session.move_link(before.links[0].id, other_ghost).unwrap();
        session.rename_package(ghost, "x".to_owned()).unwrap();
        session
            .rename_package(before.packages[0].id, String::new())
            .unwrap();
        session.merge_packages(ghost, other_ghost).unwrap();
        session.split_package(ghost).unwrap();
        session.recheck(HashSet::from([ghost])).await.unwrap();
        session
            .set_format_choice(
                FormatChoice::new(None, None, MediaContainer::Mp4, None),
                ghost,
            )
            .unwrap();

        assert_eq!(session.snapshot().unwrap(), before);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prune_older_than_uses_the_caller_supplied_now() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        origin.set_behavior(online(), &su("https://a.example.com/movie.mp4"));
        let started = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let session = session_over(&origin, Some(Arc::new(move || started)));
        session
            .ingest_urls(vec![su("https://a.example.com/movie.mp4")])
            .await
            .unwrap();

        session
            .prune_older_than(Duration::from_secs(60), started + Duration::from_secs(30))
            .unwrap();
        assert_eq!(session.snapshot().unwrap().total_count, 1, "30 s old: kept");

        session
            .prune_older_than(Duration::from_secs(60), started + Duration::from_secs(120))
            .unwrap();
        assert_eq!(
            session.snapshot().unwrap().total_count,
            0,
            "120 s old: pruned"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_zero_budget_is_an_error_not_a_panic() {
    let zero_probes = GrabberBudget {
        global_max_concurrent_probes: 0,
        ..GrabberBudget::default()
    };
    assert!(matches!(
        validate_budget(&zero_probes),
        Err(CoreError::InvalidArgument { .. })
    ));
    let refused = GrabberSession::new(GrabberConfig {
        budget: GrabberBudget {
            max_concurrent_resolves: 0,
            ..GrabberBudget::default()
        },
        deep_sniff_enabled: true,
        binary_search_paths: vec![],
        cookie_source: CookieSource::None,
        max_playlist_videos: 50,
        yt_dlp_extra_arguments: vec![],
        quality_preferences: QualityPreferences::default(),
        ffmpeg_available: true,
    });
    assert!(matches!(refused, Err(CoreError::InvalidArgument { .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_provider_closures_read_the_latest_setter_values() {
    // The session's ffmpeg and quality providers run under its lock; the
    // wrapper feeds them from stored copies. A setter call must be visible to
    // the very next read.
    let settings = Arc::new(GrabberSettings::new(QualityPreferences::default(), true));
    assert!(settings.ffmpeg_available());
    settings.set_ffmpeg_available(false);
    assert!(!settings.ffmpeg_available());
    let preferences = QualityPreferences {
        max_height: 480,
        ..QualityPreferences::default()
    };
    settings.set_quality_preferences(preferences.clone());
    assert_eq!(settings.quality_preferences(), preferences);
}
