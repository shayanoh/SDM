mod support;

use sdm_core::{AudioCodec, MediaContainer, MediaDelivery, MediaKind, VideoCodec};
use sdm_resolve::YtDlpParser;
use support::fixture_dump;

#[test]
fn maps_muxed_video_dump_to_resolved_media_with_direct_formats_only() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_muxed")).unwrap();
    assert_eq!(media.video_id, "dQw4w9WgXcQ");
    assert_eq!(media.extractor, "youtube");
    assert_eq!(media.duration_seconds, Some(213.0));
    let mut ids: Vec<&str> = media.formats.iter().map(|f| f.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["137", "140", "18", "248", "251", "399"]);
}

#[test]
fn classifies_stream_kinds() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_muxed")).unwrap();
    let kind = |id: &str| media.formats.iter().find(|f| f.id == id).map(|f| f.kind);
    assert_eq!(kind("137"), Some(MediaKind::VideoOnly));
    assert_eq!(kind("251"), Some(MediaKind::AudioOnly));
    assert_eq!(kind("18"), Some(MediaKind::Progressive));
}

#[test]
fn maps_codecs_and_containers() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_muxed")).unwrap();
    let find = |id: &str| media.formats.iter().find(|f| f.id == id).unwrap();
    assert_eq!(find("399").vcodec, Some(VideoCodec::Av1));
    assert_eq!(find("248").vcodec, Some(VideoCodec::Vp9));
    assert_eq!(find("137").vcodec, Some(VideoCodec::H264));
    assert_eq!(find("251").acodec, Some(AudioCodec::Opus));
    assert_eq!(find("140").acodec, Some(AudioCodec::Aac));
    assert_eq!(find("248").container, MediaContainer::Webm);
}

#[test]
fn progressive_only_dump_still_resolves() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_progressive_only")).unwrap();
    assert_eq!(media.formats.len(), 1);
    assert_eq!(media.formats[0].kind, MediaKind::Progressive);
}

#[test]
fn delivery_tagging() {
    assert_eq!(
        YtDlpParser::delivery_for(Some("https")),
        Some(MediaDelivery::Direct)
    );
    assert_eq!(YtDlpParser::delivery_for(None), Some(MediaDelivery::Direct));
    assert_eq!(
        YtDlpParser::delivery_for(Some("https_native")),
        Some(MediaDelivery::Direct)
    );
    assert_eq!(
        YtDlpParser::delivery_for(Some("m3u8_native")),
        Some(MediaDelivery::Hls)
    );
    assert_eq!(
        YtDlpParser::delivery_for(Some("m3u8")),
        Some(MediaDelivery::Hls)
    );
    assert_eq!(
        YtDlpParser::delivery_for(Some("http_dash_segments")),
        Some(MediaDelivery::Dash)
    );
    assert_eq!(YtDlpParser::delivery_for(Some("rtmp")), None);
    assert_eq!(YtDlpParser::delivery_for(Some("ism")), None);
}

#[test]
fn hls_only_dump_maps_hls_formats_instead_of_rejecting() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_hls_only")).unwrap();
    assert!(
        media
            .formats
            .iter()
            .any(|f| f.id == "270" && f.delivery == MediaDelivery::Hls)
    );
    // The audio+video "none/none" manifest entry is still rejected.
    assert!(!media.formats.iter().any(|f| f.id == "233"));
}

#[test]
fn codec_less_tube_formats_are_not_rejected() {
    // xnxx / xvideos omit vcodec/acodec even for downloadable muxed streams.
    let media = YtDlpParser::resolved_media(&fixture_dump("video_tube_no_codecs")).unwrap();
    assert_eq!(media.formats.len(), 5);
    // `low` / `high`: no height, ext mp4, direct → progressive direct.
    let low = media.formats.iter().find(|f| f.id == "low").unwrap();
    assert_eq!(low.kind, MediaKind::Progressive);
    assert_eq!(low.delivery, MediaDelivery::Direct);
    // hls-1080p: has height, m3u8 → progressive hls.
    let hd = media.formats.iter().find(|f| f.id == "hls-1080p").unwrap();
    assert_eq!(hd.kind, MediaKind::Progressive);
    assert_eq!(hd.delivery, MediaDelivery::Hls);
}

#[test]
fn direct_dump_tags_every_format_direct() {
    let media = YtDlpParser::resolved_media(&fixture_dump("video_direct_vimeo")).unwrap();
    assert!(
        media
            .formats
            .iter()
            .all(|f| f.delivery == MediaDelivery::Direct)
    );
    assert_eq!(media.formats.len(), 3);
}

// Not ported from Swift: pins the `JSONDecoder` tolerance the dump types
// reproduce (floats for integers, null, unknown keys, type mismatches).
#[test]
fn decoding_matches_json_decoder_tolerance() {
    use sdm_resolve::YtDlpDump;
    let dump = YtDlpDump::from_json(
        br#"{"id":"a","unknown":{"x":1},"duration":10,"title":null,
            "formats":[{"format_id":"1","height":1080.0,"width":null,"filesize":2e3,
                        "filesize_approx":12345678.0,"tbr":128,"url":"https://x/y"}]}"#,
    )
    .unwrap();
    assert_eq!(dump.duration, Some(10.0));
    assert_eq!(dump.title, None);
    let f = &dump.formats.unwrap()[0];
    assert_eq!(f.height, Some(1080));
    assert_eq!(f.width, None);
    assert_eq!(f.filesize, Some(2000));
    assert_eq!(f.filesize_approx, Some(12_345_678));
    assert_eq!(f.tbr, Some(128.0));
    for bad in [
        r#"{"formats":[{"height":1080.5}]}"#,
        r#"{"formats":[{"height":"1080"}]}"#,
        r#"{"formats":[{"height":true}]}"#,
        r#"{"title":5}"#,
        r#"{"formats":[null]}"#,
        r#" []"#,
    ] {
        assert!(YtDlpDump::from_json(bad.as_bytes()).is_err(), "{bad}");
    }
}
