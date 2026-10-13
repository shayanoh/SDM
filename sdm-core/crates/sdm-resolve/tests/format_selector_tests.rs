use sdm_core::{
    AudioCodec, MediaContainer, MediaDelivery, MediaFormat, MediaKind, QualityPreferences,
    ResolvedMedia, SourceUrl, VideoCodec,
};
use sdm_resolve::FormatSelector;

fn url(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

fn vf(id: &str, height: i64, vcodec: VideoCodec, container: MediaContainer) -> MediaFormat {
    vf_with(id, height, vcodec, container, 1000, 1000.0, false, None)
}

#[allow(clippy::too_many_arguments)]
fn vf_with(
    id: &str,
    height: i64,
    vcodec: VideoCodec,
    container: MediaContainer,
    size: i64,
    tbr: f64,
    progressive: bool,
    acodec: Option<AudioCodec>,
) -> MediaFormat {
    MediaFormat::new(
        id,
        if progressive {
            MediaKind::Progressive
        } else {
            MediaKind::VideoOnly
        },
        Some(height),
        Some(height * 16 / 9),
        Some(vcodec),
        acodec,
        container,
        Some(size),
        None,
        Some(tbr),
        url(&format!("https://gv/{id}")),
    )
}

fn af(id: &str, acodec: AudioCodec, container: MediaContainer) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(acodec),
        container,
        Some(100),
        None,
        Some(128.0),
        url(&format!("https://gv/{id}")),
    )
}

fn media(formats: Vec<MediaFormat>) -> ResolvedMedia {
    ResolvedMedia::new("youtube", "abc", "T", Some(10.0), formats)
}

fn hls(id: &str, h: i64) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::VideoOnly,
        Some(h),
        None,
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        None,
        None,
        Some(h as f64),
        url(&format!("https://x/{id}.m3u8")),
    )
    .with_delivery(MediaDelivery::Hls)
}

fn pick(m: &ResolvedMedia) -> Option<sdm_core::FormatChoice> {
    FormatSelector::pick(m, &QualityPreferences::default())
}

fn video_id(choice: &Option<sdm_core::FormatChoice>) -> Option<&str> {
    choice.as_ref()?.video.as_ref().map(|v| v.id.as_str())
}

#[test]
fn highest_quality_wins_regardless_of_delivery() {
    // A 1080p HLS variant beats a 720p direct one within the 1080 ceiling.
    let m = media(vec![
        vf("137", 720, VideoCodec::H264, MediaContainer::Mp4),
        af("140", AudioCodec::Aac, MediaContainer::M4a),
        hls("270", 1080),
    ]);
    let choice = pick(&m);
    assert!(choice.as_ref().unwrap().is_wholesale());
    assert_eq!(video_id(&choice), Some("270"));
}

#[test]
fn direct_wins_over_hls_at_equal_resolution() {
    let m = media(vec![
        vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("140", AudioCodec::Aac, MediaContainer::M4a),
        hls("270", 1080),
    ]);
    let choice = pick(&m);
    assert!(!choice.as_ref().unwrap().is_wholesale());
    assert_eq!(video_id(&choice), Some("137"));
}

#[test]
fn direct_wins_over_hls_at_equal_resolution_even_with_a_worse_codec() {
    // Direct H.264 1080p vs streaming AV1 1080p → the resumable one wins.
    let hls_av1 = MediaFormat::new(
        "av1hls",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::Av1),
        None,
        MediaContainer::Mp4,
        None,
        None,
        Some(2000.0),
        url("https://x/av1.m3u8"),
    )
    .with_delivery(MediaDelivery::Hls);
    let m = media(vec![
        vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("140", AudioCodec::Aac, MediaContainer::M4a),
        hls_av1,
    ]);
    let choice = pick(&m);
    assert!(!choice.as_ref().unwrap().is_wholesale());
    assert_eq!(video_id(&choice), Some("137"));
}

#[test]
fn hls_only_media_yields_wholesale_choice() {
    let m = media(vec![hls("270", 1080), hls("232", 720)]);
    let choice = pick(&m);
    let c = choice.as_ref().unwrap();
    assert!(c.is_wholesale());
    assert!(
        c.wholesale_selector
            .as_ref()
            .unwrap()
            .contains("height<=1080")
    );
    assert_eq!(c.output_container, MediaContainer::Mp4);
    assert_eq!(video_id(&choice), Some("270"));
}

#[test]
fn hls_above_the_cap_still_downloads() {
    let m = media(vec![hls("2160", 2160)]);
    assert!(pick(&m).unwrap().is_wholesale());
}

#[test]
fn nothing_usable_yields_nil() {
    assert_eq!(pick(&media(vec![])), None);
}

// Codec-less tube-site formats (xnxx / xvideos): low/high direct MP4 with no
// height, plus HLS variants that do carry height.
fn nc(id: &str, height: Option<i64>, tbr: Option<f64>, is_hls: bool) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::Progressive,
        height,
        None,
        None,
        None,
        MediaContainer::Mp4,
        None,
        None,
        tbr,
        url(&format!(
            "https://x/{id}{}",
            if is_hls { "/i.m3u8" } else { ".mp4" }
        )),
    )
    .with_delivery(if is_hls {
        MediaDelivery::Hls
    } else {
        MediaDelivery::Direct
    })
}

#[test]
fn codec_less_hls_wins_over_heightless_direct() {
    let m = media(vec![
        nc("low", None, None, false),
        nc("high", None, None, false),
        nc("hls-480", Some(480), Some(764.0), true),
        nc("hls-1080", Some(1080), Some(2760.0), true),
    ]);
    let choice = pick(&m);
    assert!(choice.as_ref().unwrap().is_wholesale());
    assert_eq!(video_id(&choice), Some("hls-1080"));
}

#[test]
fn codec_less_direct_progressive_is_chosen_directly_when_best() {
    let m = media(vec![nc("mp4-720", Some(720), Some(1500.0), false)]);
    let choice = pick(&m);
    let c = choice.as_ref().unwrap();
    assert!(!c.is_wholesale());
    assert_eq!(video_id(&choice), Some("mp4-720"));
    assert_eq!(c.audio, None);
}

#[test]
fn picks_highest_resolution_then_codec_then_container() {
    let m = media(vec![
        vf("v720av1", 720, VideoCodec::Av1, MediaContainer::Webm),
        vf("v1080vp9", 1080, VideoCodec::Vp9, MediaContainer::Webm),
        vf("v1080h264", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("a", AudioCodec::Opus, MediaContainer::Webm),
    ]);
    let choice = pick(&m);
    let c = choice.as_ref().unwrap();
    assert_eq!(video_id(&choice), Some("v1080vp9"));
    assert_eq!(c.audio.as_ref().map(|a| a.id.as_str()), Some("a"));
    assert_eq!(c.output_container, MediaContainer::Webm);
    assert!(c.requires_mux());
}

#[test]
fn progressive_format_needs_no_audio_and_no_mux() {
    let m = media(vec![vf_with(
        "prog",
        720,
        VideoCodec::H264,
        MediaContainer::Mp4,
        1000,
        1000.0,
        true,
        Some(AudioCodec::Aac),
    )]);
    let choice = pick(&m);
    let c = choice.as_ref().unwrap();
    assert_eq!(video_id(&choice), Some("prog"));
    assert_eq!(c.audio, None);
    assert!(!c.requires_mux());
    assert_eq!(c.output_container, MediaContainer::Mp4);
}

#[test]
fn max_height_cap_drops_higher_formats() {
    let prefs = QualityPreferences {
        max_height: 720,
        ..QualityPreferences::default()
    };
    let m = media(vec![
        vf("v2160", 2160, VideoCodec::Av1, MediaContainer::Mp4),
        vf("v720", 720, VideoCodec::H264, MediaContainer::Mp4),
        af("a", AudioCodec::Aac, MediaContainer::M4a),
    ]);
    let choice = FormatSelector::pick(&m, &prefs);
    assert_eq!(video_id(&choice), Some("v720"));
}

#[test]
fn codec_allowlist_excluding_top_format() {
    let prefs = QualityPreferences {
        video_codecs: [VideoCodec::H264].into_iter().collect(),
        ..QualityPreferences::default()
    };
    let m = media(vec![
        vf("av1", 1080, VideoCodec::Av1, MediaContainer::Mp4),
        vf("h264", 1080, VideoCodec::H264, MediaContainer::Mp4),
        af("a", AudioCodec::Aac, MediaContainer::M4a),
    ]);
    let choice = FormatSelector::pick(&m, &prefs);
    assert_eq!(video_id(&choice), Some("h264"));
}

#[test]
fn video_only_with_no_eligible_audio_returns_nil() {
    let prefs = QualityPreferences {
        audio_codecs: [AudioCodec::Opus].into_iter().collect(),
        ..QualityPreferences::default()
    };
    let m = media(vec![
        vf("v", 1080, VideoCodec::Av1, MediaContainer::Webm),
        af("a", AudioCodec::Aac, MediaContainer::M4a),
    ]);
    assert_eq!(FormatSelector::pick(&m, &prefs), None);
}

#[test]
fn no_eligible_video_but_videos_exist_returns_nil() {
    let prefs = QualityPreferences {
        max_height: 144,
        ..QualityPreferences::default()
    };
    let m = media(vec![
        vf("v1080", 1080, VideoCodec::Av1, MediaContainer::Webm),
        af("a", AudioCodec::Opus, MediaContainer::Webm),
    ]);
    assert_eq!(FormatSelector::pick(&m, &prefs), None);
}

#[test]
fn audio_only_media_picks_best_audio() {
    let m = media(vec![
        af("aac", AudioCodec::Aac, MediaContainer::M4a),
        af("opus", AudioCodec::Opus, MediaContainer::Webm),
    ]);
    let choice = pick(&m);
    let c = choice.as_ref().unwrap();
    assert_eq!(c.video, None);
    assert_eq!(c.audio.as_ref().map(|a| a.id.as_str()), Some("opus"));
    assert_eq!(c.output_container, MediaContainer::Webm);
}

#[test]
fn estimated_bytes_nil_when_a_component_size_unknown() {
    let v_no_size = MediaFormat::new(
        "v",
        MediaKind::VideoOnly,
        Some(1080),
        Some(1920),
        Some(VideoCodec::Av1),
        None,
        MediaContainer::Webm,
        None,
        None,
        None,
        url("https://gv/v"),
    );
    let m = media(vec![
        v_no_size,
        af("a", AudioCodec::Opus, MediaContainer::Webm),
    ]);
    assert_eq!(pick(&m).unwrap().estimated_bytes, None);
}
