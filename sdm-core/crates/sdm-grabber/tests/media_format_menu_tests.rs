mod support;

use sdm_core::{
    AudioCodec, MediaContainer, MediaDelivery, MediaFormat, MediaKind, QualityPreferences,
    ResolvedMedia, VideoCodec,
};
use sdm_grabber::MediaFormatMenu;
use support::{af, su, vf};

fn media(formats: Vec<MediaFormat>) -> ResolvedMedia {
    ResolvedMedia::new("youtube", "abc", "T", Some(60.0), formats)
}

#[test]
fn matching_options_come_first_ranked_by_resolution_then_codec() {
    let m = media(vec![
        vf("v720av1", 720, VideoCodec::Av1, MediaContainer::Webm),
        vf("v1080vp9", 1080, VideoCodec::Vp9, MediaContainer::Webm),
        vf("v2160av1", 2160, VideoCodec::Av1, MediaContainer::Mp4),
        af("a", AudioCodec::Opus, MediaContainer::Webm),
    ]);
    let opts = MediaFormatMenu::options(&m, &QualityPreferences::default());
    assert_eq!(
        opts.first()
            .and_then(|o| o.choice.video.as_ref())
            .map(|v| v.id.as_str()),
        Some("v1080vp9")
    );
    let matches: Vec<bool> = opts.iter().map(|o| o.matches_preferences).collect();
    assert_eq!(matches, vec![true, true, false]);
    assert_eq!(
        opts.last()
            .and_then(|o| o.choice.video.as_ref())
            .map(|v| v.id.as_str()),
        Some("v2160av1")
    );
}

#[test]
fn progressive_formats_appear_with_no_audio_pairing() {
    let prog = MediaFormat::new(
        "18",
        MediaKind::Progressive,
        Some(360),
        Some(640),
        Some(VideoCodec::H264),
        Some(AudioCodec::Aac),
        MediaContainer::Mp4,
        Some(5_000_000),
        None,
        None,
        su("https://gv/18"),
    );
    let opts = MediaFormatMenu::options(&media(vec![prog]), &QualityPreferences::default());
    assert_eq!(opts.len(), 1);
    assert!(opts[0].choice.audio.is_none());
    assert!(opts[0].label.contains("360p"));
}

#[test]
fn a_video_only_option_with_no_eligible_audio_still_shows_marked_non_matching() {
    let prefs = QualityPreferences {
        audio_codecs: [AudioCodec::Opus].into_iter().collect(),
        ..QualityPreferences::default()
    };
    let m = media(vec![
        vf("v", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("aac140", AudioCodec::Aac, MediaContainer::M4a),
    ]);
    let opts = MediaFormatMenu::options(&m, &prefs);
    assert_eq!(opts.len(), 1);
    assert!(!opts[0].matches_preferences);
    assert_eq!(
        opts[0].choice.audio.as_ref().map(|a| a.id.as_str()),
        Some("aac140")
    );
}

#[test]
fn hls_progressive_formats_are_only_streamed_rows_never_direct() {
    // Twitch: every quality is a progressive `m3u8_native` format. None of
    // them may show up as a direct-download row whose URL is the .m3u8.
    fn hls_prog(id: &str, h: i64) -> MediaFormat {
        MediaFormat::new(
            id,
            MediaKind::Progressive,
            Some(h),
            None,
            Some(VideoCodec::H264),
            Some(AudioCodec::Aac),
            MediaContainer::Mp4,
            None,
            None,
            Some(h as f64),
            su(&format!("https://x/{id}/index.m3u8")),
        )
        .with_delivery(MediaDelivery::Hls)
    }
    let m = media(vec![
        hls_prog("1080p60", 1080),
        hls_prog("480p", 480),
        hls_prog("160p", 160),
    ]);
    let opts = MediaFormatMenu::options(&m, &QualityPreferences::default());

    assert_eq!(opts.len(), 3);
    assert!(opts.iter().all(|o| o.is_wholesale));
    assert!(opts.iter().all(|o| o.choice.wholesale_selector.is_some()));
    // The selector is yt-dlp's format id, not a URL.
    assert!(
        opts.iter()
            .any(|o| o.choice.wholesale_selector.as_deref() == Some("1080p60"))
    );
}

#[test]
fn hls_variants_appear_as_streamed_options_after_direct_ones() {
    let hls1080 = MediaFormat::new(
        "270",
        MediaKind::VideoOnly,
        Some(1080),
        None,
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        None,
        None,
        Some(3000.0),
        su("https://x/270.m3u8"),
    )
    .with_delivery(MediaDelivery::Hls);
    let m = media(vec![
        vf("v", 720, VideoCodec::H264, MediaContainer::Mp4),
        af("a", AudioCodec::Aac, MediaContainer::M4a),
        hls1080,
    ]);
    let opts = MediaFormatMenu::options(&m, &QualityPreferences::default());
    assert_eq!(opts.first().map(|o| o.is_wholesale), Some(false));
    assert_eq!(opts.last().map(|o| o.is_wholesale), Some(true));
    assert_eq!(
        opts.last()
            .and_then(|o| o.choice.wholesale_selector.as_deref()),
        Some("270+ba/b")
    );
    assert!(opts.last().is_some_and(|o| o.label.contains("streamed")));
}

#[test]
fn label_marks_approximate_sizes() {
    let v = MediaFormat::new(
        "v",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::Av1),
        None,
        MediaContainer::Webm,
        None,
        Some(80_000_000),
        Some(3000.0),
        su("https://gv/v"),
    );
    let opts = MediaFormatMenu::options(
        &media(vec![v, af("a", AudioCodec::Opus, MediaContainer::Webm)]),
        &QualityPreferences::default(),
    );
    assert!(opts[0].label.contains('~'));
}
