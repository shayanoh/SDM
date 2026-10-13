mod support;

use std::sync::Arc;

use sdm_core::{
    AudioCodec, FormatChoice, MediaContainer, MediaFormat, MediaKind, ResolveError, ResolvedMedia,
    ResolvedTarget, VideoCodec,
};
use sdm_grabber::{FakeProbeOrigin, GrabberSession, LinkProber, MediaRow, MediaRowState};
use support::{FakeLinkResolver, af, single_media, su, vf, within};

fn make_session(resolver: FakeLinkResolver) -> GrabberSession {
    GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
        .resolver(Arc::new(resolver))
        .build()
}

fn pair() -> Vec<MediaFormat> {
    vec![
        vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("140", AudioCodec::Aac, MediaContainer::M4a),
    ]
}

#[test]
fn media_row_falls_back_to_url_title_before_resolving() {
    let row = MediaRow::new(su("https://youtu.be/dQw4w9WgXcQ"));
    assert_eq!(row.state, MediaRowState::Resolving);
    assert_eq!(row.title, "dQw4w9WgXcQ");
    assert_eq!(row.display_filename(), "dQw4w9WgXcQ");
    assert_eq!(row.combined_bytes(), None);
}

#[test]
fn media_row_display_filename_uses_title_video_id_and_container() {
    let v = MediaFormat::new(
        "137",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        Some(100),
        None,
        None,
        su("https://gv/v"),
    );
    let a = MediaFormat::new(
        "140",
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(AudioCodec::Aac),
        MediaContainer::M4a,
        Some(10),
        None,
        None,
        su("https://gv/a"),
    );
    let mut row = MediaRow::new(su("https://youtu.be/abc"));
    row.media = Some(ResolvedMedia::new(
        "youtube",
        "abc",
        "Rick Astley / Never Gonna",
        Some(213.0),
        vec![v.clone(), a.clone()],
    ));
    row.choice = Some(FormatChoice::new(
        Some(v),
        Some(a),
        MediaContainer::Mp4,
        Some(110),
    ));
    row.state = MediaRowState::Resolved;
    assert_eq!(row.display_filename(), "Rick Astley Never Gonna [abc].mp4");
    assert_eq!(row.combined_bytes(), Some(110));
}

#[test]
fn package_candidate_note_defaults_nil() {
    use sdm_grabber::PackageCandidate;
    assert_eq!(PackageCandidate::new("P", vec![]).note, None);
    assert_eq!(
        PackageCandidate::new("P", vec![])
            .with_note("50 of 320 videos")
            .note
            .as_deref(),
        Some("50 of 320 videos")
    );
}

// MARK: - Routing (Task 2)

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_you_tube_url_becomes_a_resolved_media_row() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Ok(single_media("abc", "Song", pair()))
        }));
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        let snap = session.snapshot();
        assert!(snap.links.is_empty());
        assert_eq!(snap.media_rows.len(), 1);
        assert_eq!(snap.media_rows[0].state, MediaRowState::Resolved);
        let choice = snap.media_rows[0].choice.as_ref().unwrap();
        assert_eq!(choice.video.as_ref().map(|v| v.id.as_str()), Some("137"));
        assert_eq!(choice.audio.as_ref().map(|a| a.id.as_str()), Some("140"));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_non_you_tube_url_stays_on_the_http_probe_path() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Ok(ResolvedTarget::Single(ResolvedMedia::new(
                "x",
                "x",
                "x",
                None,
                vec![],
            )))
        }));
        session
            .ingest_urls(&[su("https://example.com/a.mp4")])
            .await;
        let snap = session.snapshot();
        assert!(snap.media_rows.is_empty());
        assert_eq!(snap.links.len(), 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_pick_returning_nil_leaves_the_row_unselected() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Ok(single_media(
                "abc",
                "Song",
                vec![vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4)],
            ))
        }));
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Unselected
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unsupported_resolve_marks_the_row_unsupported() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| Err(ResolveError::Unsupported)));
        session.ingest_urls(&[su("https://youtu.be/live")]).await;
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Unsupported
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_binary_marks_the_row_needs_yt_dlp() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| Err(ResolveError::BinaryMissing)));
        session.ingest_urls(&[su("https://youtu.be/x")]).await;
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::NeedsYtDlp
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mux_choice_with_no_ffmpeg_marks_the_row_needs_ffmpeg() {
    within(async {
        let resolver = FakeLinkResolver::new(|_| Ok(single_media("abc", "Song", pair())));
        let session =
            GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
                .resolver(Arc::new(resolver))
                .ffmpeg_available(|| false)
                .build();
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::NeedsFfmpeg
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manually_choosing_a_format_resolves_an_unselected_row() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Ok(single_media(
                "abc",
                "Song",
                vec![vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4)],
            ))
        }));
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        let id = session.snapshot().media_rows[0].id;
        session.set_format_choice(
            FormatChoice::new(
                Some(vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4)),
                None,
                MediaContainer::Mp4,
                Some(1000),
            ),
            id,
        );
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Resolved
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_yt_dlp_failure_surfaces_the_real_stderr_on_the_row() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Err(ResolveError::YtDlpFailed {
                stderr_tail: "ERROR: [youtube] abc: The page needs to be reloaded.".into(),
            })
        }));
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        let MediaRowState::Failed(reason) = session.snapshot().media_rows[0].state.clone() else {
            panic!("expected .failed");
        };
        assert!(reason.contains("The page needs to be reloaded"));
        assert!(reason.to_lowercase().contains("cookies")); // the prepended hint
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_generic_yt_dlp_failure_shows_the_stderr_verbatim() {
    within(async {
        let session = make_session(FakeLinkResolver::new(|_| {
            Err(ResolveError::YtDlpFailed {
                stderr_tail: "ERROR: HTTP Error 429: Too Many Requests".into(),
            })
        }));
        session.ingest_urls(&[su("https://youtu.be/xyz")]).await;
        let MediaRowState::Failed(reason) = session.snapshot().media_rows[0].state.clone() else {
            panic!("expected .failed");
        };
        assert_eq!(reason, "ERROR: HTTP Error 429: Too Many Requests");
    })
    .await;
}

/// A panic that unwinds while the session's lock is held (here, from the
/// ffmpeg provider, which the session calls under its lock) must not disable
/// the session: the polled `snapshot()` and later edits keep working.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_under_the_session_lock_does_not_disable_the_session() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicBool, Ordering};

    within(async {
        let explode = Arc::new(AtomicBool::new(false));
        let provider_flag = Arc::clone(&explode);
        let session =
            GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
                .resolver(Arc::new(FakeLinkResolver::new(|_| {
                    Ok(single_media("abc", "Song", pair()))
                })))
                .ffmpeg_available(move || {
                    assert!(!provider_flag.load(Ordering::SeqCst), "provider exploded");
                    true
                })
                .build();
        session.ingest_urls(&[su("https://youtu.be/abc")]).await;
        let id = session.snapshot().media_rows[0].id;
        let muxed = FormatChoice::new(
            Some(vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4)),
            Some(af("140", AudioCodec::Aac, MediaContainer::M4a)),
            MediaContainer::Mp4,
            Some(1100),
        );

        explode.store(true, Ordering::SeqCst);
        let result = catch_unwind(AssertUnwindSafe(|| {
            session.set_format_choice(muxed.clone(), id);
        }));
        assert!(result.is_err(), "the provider must have panicked");
        explode.store(false, Ordering::SeqCst);

        // The lock was poisoned by that unwind; the session still answers.
        assert_eq!(session.snapshot().media_rows.len(), 1);
        session.set_format_choice(muxed, id);
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Resolved
        );
    })
    .await;
}
