mod support;

use sdm_core::{
    AudioCodec, FormatChoice, MediaContainer, MediaDelivery, MediaFormat, MediaKind, ResolvedMedia,
    VideoCodec,
};
use sdm_grabber::MediaMetadata;
use support::su;

#[allow(clippy::too_many_arguments)]
fn vfmt(
    id: &str,
    h: i64,
    v: VideoCodec,
    c: MediaContainer,
    tbr: f64,
    delivery: MediaDelivery,
) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::VideoOnly,
        Some(h),
        Some(h * 16 / 9),
        Some(v),
        None,
        c,
        None,
        None,
        if tbr == 0.0 { None } else { Some(tbr) },
        su(&format!("https://x/{id}")),
    )
    .with_delivery(delivery)
}

fn afmt(id: &str, a: AudioCodec, tbr: f64) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(a),
        MediaContainer::M4a,
        None,
        None,
        Some(tbr),
        su(&format!("https://x/{id}")),
    )
}

fn media(formats: Vec<MediaFormat>, extractor: &str, duration: Option<f64>) -> ResolvedMedia {
    ResolvedMedia::new(extractor, "abc", "Clip", duration, formats)
}

#[test]
fn describes_a_direct_mux_download() {
    let v = vfmt(
        "137",
        1080,
        VideoCodec::H264,
        MediaContainer::Mp4,
        4000.0,
        MediaDelivery::Direct,
    );
    let a = afmt("140", AudioCodec::Aac, 130.0);
    let choice = FormatChoice::new(Some(v.clone()), Some(a.clone()), MediaContainer::Mp4, None);
    let s = MediaMetadata::describe(&choice, &media(vec![v, a], "youtube", Some(213.0)));
    assert_eq!(
        s,
        "Direct · 1080p · h264 · aac · mp4 · 4.1 Mbps · 3:33 · youtube"
    );
}

#[test]
fn describes_a_streaming_wholesale_download() {
    let v = vfmt(
        "270",
        1080,
        VideoCodec::H264,
        MediaContainer::Mp4,
        2760.0,
        MediaDelivery::Hls,
    );
    let choice = FormatChoice::new(Some(v.clone()), None, MediaContainer::Mp4, None)
        .with_wholesale_selector("bv*+ba/b");
    let s = MediaMetadata::describe(&choice, &media(vec![v], "twitch", Some(3600.0)));
    assert_eq!(
        s,
        "Streaming · 1080p · h264 · mp4 · 2.8 Mbps · 1:00:00 · twitch"
    );
}

#[test]
fn omits_unknown_fields_and_generic_extractor() {
    let v = MediaFormat::new(
        "x",
        MediaKind::Progressive,
        Some(720),
        None,
        None,
        None,
        MediaContainer::Mp4,
        None,
        None,
        None,
        su("https://x/x"),
    );
    let choice = FormatChoice::new(Some(v.clone()), None, MediaContainer::Mp4, None);
    let s = MediaMetadata::describe(&choice, &media(vec![v], "generic", None));
    assert_eq!(s, "Direct · 720p · mp4");
}
