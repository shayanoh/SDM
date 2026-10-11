use sdm_core::{
    AudioCodec, FormatChoice, MediaContainer, MediaDelivery, MediaFormat, MediaKind,
    QualityPreferences, SourceUrl, VideoCodec,
};
use std::collections::HashSet;

#[allow(clippy::too_many_arguments)]
fn fmt(
    id: &str,
    kind: MediaKind,
    height: Option<i64>,
    width: Option<i64>,
    vcodec: Option<VideoCodec>,
    acodec: Option<AudioCodec>,
    container: MediaContainer,
    filesize: Option<i64>,
    filesize_approx: Option<i64>,
    tbr: Option<f64>,
    url: &str,
) -> MediaFormat {
    MediaFormat::new(
        id,
        kind,
        height,
        width,
        vcodec,
        acodec,
        container,
        filesize,
        filesize_approx,
        tbr,
        SourceUrl::new(url).unwrap(),
    )
}

#[test]
fn video_codec_priority_orders_av1_first_h264_last() {
    assert!(VideoCodec::Av1.rank() < VideoCodec::Vp9.rank());
    assert!(VideoCodec::Vp9.rank() < VideoCodec::H264.rank());
    assert!(VideoCodec::H264.rank() < VideoCodec::Other("theora".into()).rank());
}

#[test]
fn container_priority_orders_mp4_before_webm() {
    assert!(MediaContainer::Mp4.rank() < MediaContainer::Webm.rank());
    assert!(MediaContainer::Webm.rank() < MediaContainer::Other("mkv".into()).rank());
}

#[test]
fn audio_codec_priority_orders_opus_before_aac() {
    assert!(AudioCodec::Opus.rank() < AudioCodec::Aac.rank());
}

#[test]
fn filesize_effective_prefers_exact_then_approx_then_nil() {
    let exact = fmt(
        "137",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        Some(100),
        Some(90),
        Some(4000.0),
        "https://x/1",
    );
    assert_eq!(exact.filesize_effective(), Some(100));
    assert!(!exact.is_approximate_size());

    let approx = fmt(
        "248",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::Vp9),
        None,
        MediaContainer::Webm,
        None,
        Some(90),
        Some(3800.0),
        "https://x/2",
    );
    assert_eq!(approx.filesize_effective(), Some(90));
    assert!(approx.is_approximate_size());

    let unknown = fmt(
        "600",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::Av1),
        None,
        MediaContainer::Mp4,
        None,
        None,
        None,
        "https://x/3",
    );
    assert_eq!(unknown.filesize_effective(), None);
}

#[test]
fn media_format_defaults_to_direct_delivery() {
    let f = fmt(
        "1",
        MediaKind::Progressive,
        Some(720),
        Some(1280),
        Some(VideoCodec::H264),
        Some(AudioCodec::Aac),
        MediaContainer::Mp4,
        Some(100),
        None,
        None,
        "https://x/y",
    );
    assert_eq!(f.delivery, MediaDelivery::Direct);
    assert!(f.is_direct());
}

#[test]
fn media_format_carries_explicit_delivery() {
    let f = fmt(
        "2",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        None,
        None,
        None,
        "https://x/m.m3u8",
    )
    .with_delivery(MediaDelivery::Hls);
    assert_eq!(f.delivery, MediaDelivery::Hls);
    assert!(!f.is_direct());
}

#[test]
fn media_container_file_extension() {
    assert_eq!(MediaContainer::Mp4.file_extension(), "mp4");
    assert_eq!(MediaContainer::Other("mkv".into()).file_extension(), "mkv");
}

#[test]
fn default_quality_preferences_accept_everything_up_to_1080() {
    let prefs = QualityPreferences::default();
    assert_eq!(prefs.max_height, 1080);
    assert_eq!(
        prefs.video_codecs,
        HashSet::from([VideoCodec::Av1, VideoCodec::Vp9, VideoCodec::H264])
    );
    assert_eq!(
        prefs.containers,
        HashSet::from([MediaContainer::Mp4, MediaContainer::Webm])
    );
    assert_eq!(
        prefs.audio_codecs,
        HashSet::from([AudioCodec::Opus, AudioCodec::Aac])
    );
}

#[test]
fn format_choice_requires_mux_only_when_both_streams_present() {
    let v = fmt(
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
        "https://x/v",
    );
    let a = fmt(
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
        "https://x/a",
    );
    let muxed = FormatChoice::new(Some(v.clone()), Some(a), MediaContainer::Mp4, Some(110));
    assert!(muxed.requires_mux());
    assert_eq!(muxed.format_ids(), ["137", "140"]);

    let progressive = FormatChoice::new(Some(v), None, MediaContainer::Mp4, Some(100));
    assert!(!progressive.requires_mux());
    assert_eq!(progressive.format_ids(), ["137"]);
}
