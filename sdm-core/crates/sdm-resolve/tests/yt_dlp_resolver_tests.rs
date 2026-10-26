mod support;

use std::sync::Arc;

use sdm_core::{LinkResolver, MediaDelivery, ResolveError, ResolvedTarget, SourceUrl};
use sdm_resolve::testing::{FakeProcessRunner, fail_default, ok};
use sdm_resolve::{BinaryLocator, CookieSource, YtDlpResolver};
use support::fixture_data;
use tokio_util::sync::CancellationToken;

fn make_locator(has_yt_dlp: bool) -> Arc<BinaryLocator> {
    Arc::new(
        BinaryLocator::new()
            .with_search_paths(vec!["/bin".into()])
            .with_is_executable(move |p| has_yt_dlp && p.to_str() == Some("/bin/yt-dlp")),
    )
}

fn u(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

fn resolver(runner: FakeProcessRunner, has_yt_dlp: bool) -> YtDlpResolver {
    YtDlpResolver::with_defaults(Arc::new(runner), make_locator(has_yt_dlp))
}

const AGE_RESTRICTED_STDERR: &str = "ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.";
const PRIVATE_STDERR: &str =
    "ERROR: [youtube] def: Private video. Sign in if you've been granted access to this video";

// MARK: - can_handle

#[test]
fn can_handle_uses_site_registry() {
    let r = resolver(FakeProcessRunner::new(), true);
    assert!(r.can_handle(&u("https://www.youtube.com/watch?v=abc")));
    assert!(r.can_handle(&u("https://youtu.be/abc")));
    assert!(r.can_handle(&u("https://music.youtube.com/watch?v=abc")));
    assert!(r.can_handle(&u("https://vimeo.com/12345")));
    assert!(r.can_handle(&u("https://www.tiktok.com/@a/video/1")));
    assert!(r.can_handle(&u("https://www.xvideos.com/video1/x")));
    assert!(!r.can_handle(&u("https://example.com/video.mp4")));
    assert!(!r.can_handle(&u("ftp://youtube.com/x")));
}

// MARK: - resolve, single video

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resolve_single_video_returns_resolved_media() {
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![("-J".into(), ok(fixture_data("video_muxed")))];
        r
    });
    let r = YtDlpResolver::with_defaults(runner.clone(), make_locator(true));
    let target = r
        .resolve(&u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
        .await
        .unwrap();
    let ResolvedTarget::Single(media) = target else {
        panic!("expected .single");
    };
    assert_eq!(media.video_id, "dQw4w9WgXcQ");
    assert!(media.formats.iter().any(|f| f.id == "399"));
    let calls = runner.calls();
    let call = calls.first().expect("a call");
    assert!(call.arguments.contains(&"--no-playlist".to_string()));
    assert!(
        call.arguments
            .contains(&"https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resolve_without_yt_dlp_throws_binary_missing() {
    let r = resolver(FakeProcessRunner::new(), false);
    assert_eq!(
        r.resolve(&u("https://youtu.be/abc")).await.unwrap_err(),
        ResolveError::BinaryMissing
    );
}

async fn resolve_error_for(url: &str, stderr: &str) -> ResolveError {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("-J".into(), fail_default(stderr))];
    resolver(runner, true).resolve(&u(url)).await.unwrap_err()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn age_restricted_video_throws_auth_required() {
    assert_eq!(
        resolve_error_for("https://youtu.be/abc", AGE_RESTRICTED_STDERR).await,
        ResolveError::AuthRequired
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn generic_login_stderr_is_auth_required() {
    assert_eq!(
        resolve_error_for(
            "https://vimeo.com/1",
            "ERROR: [vimeo] 1: This video requires you to log in."
        )
        .await,
        ResolveError::AuthRequired
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn geo_restriction_stderr_is_unavailable() {
    assert_eq!(
        resolve_error_for(
            "https://vimeo.com/1",
            "ERROR: The uploader has not made this video available in your country."
        )
        .await,
        ResolveError::Unavailable
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drm_stderr_is_drm_protected() {
    assert_eq!(
        resolve_error_for(
            "https://x.com/1",
            "ERROR: [generic] x: This video is DRM protected."
        )
        .await,
        ResolveError::DrmProtected
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_video_throws_private_video() {
    assert_eq!(
        resolve_error_for("https://youtu.be/def", PRIVATE_STDERR).await,
        ResolveError::PrivateVideo
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_only_video_resolves_with_hls_formats() {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("-J".into(), ok(fixture_data("video_hls_only")))];
    let target = resolver(runner, true)
        .resolve(&u("https://youtu.be/hls"))
        .await
        .unwrap();
    let ResolvedTarget::Single(media) = target else {
        panic!("expected .single");
    };
    assert!(
        media
            .formats
            .iter()
            .any(|f| f.delivery == MediaDelivery::Hls)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_non_you_tube_site_resolves() {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("-J".into(), ok(fixture_data("video_direct_vimeo")))];
    let target = resolver(runner, true)
        .resolve(&u("https://vimeo.com/12345"))
        .await
        .unwrap();
    let ResolvedTarget::Single(media) = target else {
        panic!("expected .single");
    };
    assert!(
        media
            .formats
            .iter()
            .all(|f| f.delivery == MediaDelivery::Direct)
    );
    assert_eq!(media.extractor, "vimeo");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flat_playlist_entries_carry_their_own_source_url() {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![(
        "--flat-playlist".into(),
        ok(fixture_data("playlist_soundcloud")),
    )];
    let target = resolver(runner, true)
        .resolve(&u("https://soundcloud.com/artist/sets/mix"))
        .await
        .unwrap();
    let ResolvedTarget::Playlist { title, entries, .. } = target else {
        panic!("expected .playlist");
    };
    assert_eq!(title, "Summer Mix");
    assert_eq!(
        entries
            .first()
            .and_then(|e| e.source_url.as_ref())
            .map(|s| s.as_str()),
        Some("https://soundcloud.com/artist/track-1")
    );
    assert_eq!(entries.len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_resolve_that_is_actually_a_playlist_routes_to_playlist() {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("-J".into(), ok(fixture_data("single_but_type_playlist")))];
    // A plain single-video URL — the resolver only learns it is a playlist
    // from the dump's `_type`.
    let target = resolver(runner, true)
        .resolve(&u("https://vimeo.com/999999"))
        .await
        .unwrap();
    let ResolvedTarget::Playlist { entries, .. } = target else {
        panic!("expected .playlist");
    };
    assert_eq!(
        entries
            .first()
            .and_then(|e| e.source_url.as_ref())
            .map(|s| s.as_str()),
        Some("https://vimeo.com/111111")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cookie_source_adds_arguments() {
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![("-J".into(), ok(fixture_data("video_muxed")))];
        r
    });
    let r = YtDlpResolver::new(
        runner.clone(),
        make_locator(true),
        Arc::new(|| CookieSource::Firefox),
        Arc::new(|| 50),
        Arc::new(Vec::new),
    );
    r.resolve(&u("https://youtu.be/abc")).await.unwrap();
    let calls = runner.calls();
    let call = calls.first().expect("a call");
    assert!(
        call.arguments
            .contains(&"--cookies-from-browser".to_string())
    );
    assert!(call.arguments.contains(&"firefox".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runner_timeout_becomes_resolve_timeout() {
    let mut runner = FakeProcessRunner::new();
    runner.default_output = Err(sdm_resolve::ProcessRunError::TimedOut);
    let r = resolver(runner, true);
    assert_eq!(
        r.resolve(&u("https://youtu.be/abc")).await.unwrap_err(),
        ResolveError::Timeout
    );
}

// MARK: - resolve, playlists & channels

#[test]
fn detects_playlist_and_channel_urls() {
    let r = resolver(FakeProcessRunner::new(), true);
    assert!(
        r.looks_like_playlist(
            u("https://www.youtube.com/playlist?list=PLabc")
                .as_url()
                .unwrap()
        )
    );
    assert!(
        r.looks_like_playlist(
            u("https://www.youtube.com/watch?v=abc&list=PLabc")
                .as_url()
                .unwrap()
        )
    );
    assert!(r.looks_like_playlist(u("https://www.youtube.com/@SomeCreator").as_url().unwrap()));
    assert!(
        r.looks_like_playlist(
            u("https://www.youtube.com/channel/UCxyz/videos")
                .as_url()
                .unwrap()
        )
    );
    assert!(!r.looks_like_playlist(u("https://www.youtube.com/watch?v=abc").as_url().unwrap()));
    assert!(!r.looks_like_playlist(u("https://youtu.be/abc").as_url().unwrap()));
}

fn playlist_resolver(max: usize) -> YtDlpResolver {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("--flat-playlist".into(), ok(fixture_data("playlist_flat")))];
    YtDlpResolver::new(
        Arc::new(runner),
        make_locator(true),
        Arc::new(|| CookieSource::None),
        Arc::new(move || max),
        Arc::new(Vec::new),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resolve_playlist_returns_capped_newest_entries() {
    let r = playlist_resolver(10);
    let target = r
        .resolve(&u("https://www.youtube.com/playlist?list=PLtest"))
        .await
        .unwrap();
    let ResolvedTarget::Playlist {
        title,
        entries,
        total_available,
    } = target
    else {
        panic!("expected .playlist");
    };
    assert_eq!(title, "My Test Playlist");
    assert_eq!(total_available, 12);
    assert_eq!(entries.len(), 10);
    assert_eq!(entries.first().unwrap().video_id, "vid03");
    assert_eq!(entries.last().unwrap().video_id, "vid12");
    assert!(entries.iter().all(|e| e.formats.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resolve_channel_keeps_head_entries() {
    let r = playlist_resolver(10);
    let target = r
        .resolve(&u("https://www.youtube.com/@Creator/videos"))
        .await
        .unwrap();
    let ResolvedTarget::Playlist { entries, .. } = target else {
        panic!("expected .playlist");
    };
    assert_eq!(entries.first().unwrap().video_id, "vid01");
    assert_eq!(entries.last().unwrap().video_id, "vid10");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn playlist_cap_clamps_to_floor_of_ten() {
    let r = playlist_resolver(3);
    let target = r
        .resolve(&u("https://www.youtube.com/playlist?list=PLtest"))
        .await
        .unwrap();
    let ResolvedTarget::Playlist { entries, .. } = target else {
        panic!("expected .playlist");
    };
    assert_eq!(entries.len(), 10);
}

// MARK: - refresh

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_returns_fresh_url_for_same_format() {
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![("-J".into(), ok(fixture_data("video_refresh_ok")))];
        r
    });
    let r = YtDlpResolver::with_defaults(runner.clone(), make_locator(true));
    let refreshed = r
        .refresh(
            &u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            "137",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(refreshed.format_id, "137");
    assert_eq!(refreshed.filesize, Some(118_000_000));
    // The refreshed URL is persisted, so it keeps the exact string yt-dlp
    // reported (mixed-case host `sn-FRESH`), as Swift's `URL` does.
    assert!(refreshed.url.as_str().contains("sn-FRESH"));
    let calls = runner.calls();
    let call = calls.first().expect("a call");
    assert!(
        call.arguments
            .contains(&"https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_throws_format_gone_when_format_missing() {
    let mut runner = FakeProcessRunner::new();
    runner.responses = vec![("-J".into(), ok(fixture_data("video_refresh_format_gone")))];
    let r = resolver(runner, true);
    assert_eq!(
        r.refresh(
            &u("https://youtu.be/dQw4w9WgXcQ"),
            "137",
            &CancellationToken::new()
        )
        .await
        .unwrap_err(),
        ResolveError::FormatGone
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_without_yt_dlp_throws_binary_missing() {
    let r = resolver(FakeProcessRunner::new(), false);
    assert_eq!(
        r.refresh(&u("https://youtu.be/x"), "137", &CancellationToken::new())
            .await
            .unwrap_err(),
        ResolveError::BinaryMissing
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_rejects_an_unsupported_source_url() {
    let r = resolver(FakeProcessRunner::new(), true);
    assert_eq!(
        r.refresh(
            &u("https://example.com/12345"),
            "1",
            &CancellationToken::new()
        )
        .await
        .unwrap_err(),
        ResolveError::Unsupported
    );
}

/// Not in the Swift suite: Swift lets `CancellationError` escape `refresh`;
/// the Rust twin is `ResolveError::Cancelled` when the token is cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_surfaces_cancellation_from_the_runner() {
    let mut runner = FakeProcessRunner::new();
    runner.default_output = Err(sdm_resolve::ProcessRunError::Cancelled);
    let r = resolver(runner, true);
    assert_eq!(
        r.refresh(&u("https://youtu.be/x"), "137", &CancellationToken::new())
            .await
            .unwrap_err(),
        ResolveError::Cancelled
    );
}

// MARK: - Classifier

#[test]
fn classifier_recognizes_curly_apostrophe_bot_wall() {
    let stderr = "ERROR: [youtube] abc: Sign in to confirm you\u{2019}re not a bot. Use --cookies-from-browser";
    assert_eq!(
        sdm_resolve::Classifier::error_from_stderr(stderr, 1),
        ResolveError::AuthRequired
    );
}

#[test]
fn classifier_keeps_page_needs_reloading_as_raw_failure() {
    let stderr = "ERROR: [youtube] pBLlM8ZvEQo: The page needs to be reloaded.\n";
    let result = sdm_resolve::Classifier::error_from_stderr(stderr, 1);
    let ResolveError::YtDlpFailed { stderr_tail } = result else {
        panic!("expected .ytDlpFailed, got {result:?}");
    };
    assert!(stderr_tail.contains("The page needs to be reloaded"));
}

#[test]
fn classifier_unknown_error_keeps_a_generous_stderr_tail() {
    let stderr = format!("{}ERROR: something specific happened", "x".repeat(5000));
    let ResolveError::YtDlpFailed { stderr_tail } =
        sdm_resolve::Classifier::error_from_stderr(&stderr, 1)
    else {
        panic!("expected .ytDlpFailed");
    };
    assert!(stderr_tail.contains("ERROR: something specific happened"));
    assert!(stderr_tail.chars().count() <= 2000);
}

// MARK: - extra arguments

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn splices_extra_arguments_into_single_resolve() {
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![("-J".into(), ok(fixture_data("video_muxed")))];
        r
    });
    let r = YtDlpResolver::new(
        runner.clone(),
        make_locator(true),
        Arc::new(|| CookieSource::None),
        Arc::new(|| 50),
        Arc::new(|| {
            vec![
                "--extractor-args".into(),
                "youtube:jsruntime=quickjs".into(),
                "--ffmpeg-location".into(),
                "/x/bin".into(),
            ]
        }),
    );
    r.resolve(&u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
        .await
        .unwrap();
    let calls = runner.calls();
    let call = calls.first().expect("a call");
    assert!(
        call.arguments
            .contains(&"youtube:jsruntime=quickjs".to_string())
    );
    assert!(call.arguments.contains(&"/x/bin".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn splices_extra_arguments_into_playlist_and_refresh() {
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![
            ("--flat-playlist".into(), ok(fixture_data("playlist_flat"))),
            ("-J".into(), ok(fixture_data("video_refresh_ok"))),
        ];
        r
    });
    let r = YtDlpResolver::new(
        runner.clone(),
        make_locator(true),
        Arc::new(|| CookieSource::None),
        Arc::new(|| 50),
        Arc::new(|| vec!["--sponsorblock-mark".into(), "none".into()]),
    );
    let _ = r
        .resolve(&u("https://www.youtube.com/playlist?list=PL123"))
        .await;
    let _ = r
        .refresh(
            &u("https://www.youtube.com/watch?v=abc"),
            "18",
            &CancellationToken::new(),
        )
        .await;
    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|c| c.arguments.contains(&"--sponsorblock-mark".to_string()))
    );
}

// MARK: - Complete yt-dlp argument vectors (original URL strings)

/// A URL whose host is upper-case: `url::Url` would lowercase it, Swift's
/// `absoluteString` keeps it, and that is what yt-dlp must be given.
const UPPER_HOST_VIDEO: &str = "https://WWW.YouTube.com/watch?v=AbC";
const UPPER_HOST_PLAYLIST: &str = "https://WWW.YouTube.com/playlist?list=PLtest";

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn vector_resolver(runner: Arc<FakeProcessRunner>) -> YtDlpResolver {
    YtDlpResolver::new(
        runner,
        make_locator(true),
        Arc::new(|| CookieSource::Safari),
        Arc::new(|| 50),
        Arc::new(|| strings(&["--ffmpeg-location", "/x/bin"])),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_resolve_passes_the_complete_argument_vector_with_the_original_url() {
    let mut fake = FakeProcessRunner::new();
    fake.responses = vec![("-J".into(), ok(fixture_data("video_muxed")))];
    let runner = Arc::new(fake);
    vector_resolver(runner.clone())
        .resolve(&u(UPPER_HOST_VIDEO))
        .await
        .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments,
        strings(&[
            "-J",
            "--no-warnings",
            "--no-playlist",
            "--cookies-from-browser",
            "safari",
            UPPER_HOST_VIDEO,
            "--ffmpeg-location",
            "/x/bin",
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn playlist_resolve_passes_the_complete_argument_vector_with_the_original_url() {
    let mut fake = FakeProcessRunner::new();
    fake.responses = vec![("--flat-playlist".into(), ok(fixture_data("playlist_flat")))];
    let runner = Arc::new(fake);
    vector_resolver(runner.clone())
        .resolve(&u(UPPER_HOST_PLAYLIST))
        .await
        .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments,
        strings(&[
            "-J",
            "--flat-playlist",
            "--no-warnings",
            "--cookies-from-browser",
            "safari",
            UPPER_HOST_PLAYLIST,
            "--ffmpeg-location",
            "/x/bin",
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_passes_the_complete_argument_vector_with_the_original_url() {
    let mut fake = FakeProcessRunner::new();
    fake.responses = vec![("-J".into(), ok(fixture_data("video_refresh_ok")))];
    let runner = Arc::new(fake);
    vector_resolver(runner.clone())
        .refresh(&u(UPPER_HOST_VIDEO), "137", &CancellationToken::new())
        .await
        .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments,
        strings(&[
            "-J",
            "--no-warnings",
            "--no-playlist",
            "--cookies-from-browser",
            "safari",
            UPPER_HOST_VIDEO,
            "--ffmpeg-location",
            "/x/bin",
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_url_with_no_parsed_form_is_unsupported() {
    let relative: SourceUrl = serde_json::from_str("\"a/b\"").unwrap();
    assert!(relative.as_url().is_none());
    let r = resolver(FakeProcessRunner::new(), true);
    assert!(!r.can_handle(&relative));
    assert_eq!(
        r.resolve(&relative).await.unwrap_err(),
        ResolveError::Unsupported
    );
    assert_eq!(
        r.refresh(&relative, "1", &CancellationToken::new())
            .await
            .unwrap_err(),
        ResolveError::Unsupported
    );
}

// MARK: - cookie provider

struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cookie_provider_replaces_the_source_arguments_and_the_lease_is_released_after_the_run() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let runner = Arc::new({
        let mut r = FakeProcessRunner::new();
        r.responses = vec![("-J".into(), ok(fixture_data("video_muxed")))];
        r
    });
    let released = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&released);
    let r = YtDlpResolver::new(
        runner.clone(),
        make_locator(true),
        Arc::new(|| CookieSource::Chrome),
        Arc::new(|| 50),
        Arc::new(Vec::new),
    )
    .with_cookie_provider(Arc::new(move || {
        sdm_resolve::CookieLease::with_guard(
            vec!["--cookies".into(), "/data/leases/1.txt".into()],
            Box::new(DropFlag(Arc::clone(&flag))),
        )
    }));
    r.resolve(&u("https://youtu.be/abc")).await.unwrap();
    let calls = runner.calls();
    let arguments = &calls.first().expect("a call").arguments;
    assert!(arguments.contains(&"--cookies".to_string()));
    assert!(arguments.contains(&"/data/leases/1.txt".to_string()));
    assert!(!arguments.contains(&"--cookies-from-browser".to_string()));
    assert!(
        released.load(Ordering::SeqCst),
        "lease must be dropped after the run"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_in_failure_without_the_chrome_cookie_file_points_at_the_extension() {
    let mut runner = FakeProcessRunner::new();
    runner.default_output = fail_default(AGE_RESTRICTED_STDERR);
    let r = YtDlpResolver::with_defaults(Arc::new(runner), make_locator(true))
        .with_cookie_provider(Arc::new(sdm_resolve::CookieLease::chrome_unavailable));
    let error = r.resolve(&u("https://youtu.be/abc")).await.unwrap_err();
    let ResolveError::YtDlpFailed { stderr_tail } = error else {
        panic!("expected YtDlpFailed, got {error:?}");
    };
    assert!(
        stderr_tail.contains("SDM Chrome extension"),
        "{stderr_tail}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_in_failure_with_a_cookie_file_stays_auth_required() {
    let mut runner = FakeProcessRunner::new();
    runner.default_output = fail_default(AGE_RESTRICTED_STDERR);
    let r = YtDlpResolver::with_defaults(Arc::new(runner), make_locator(true))
        .with_cookie_provider(Arc::new(|| {
            sdm_resolve::CookieLease::plain(vec!["--cookies".into(), "/x".into()])
        }));
    assert_eq!(
        r.resolve(&u("https://youtu.be/abc")).await.unwrap_err(),
        ResolveError::AuthRequired
    );
}
