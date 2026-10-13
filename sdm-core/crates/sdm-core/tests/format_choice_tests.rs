use sdm_core::{
    AudioCodec, FormatChoice, MediaContainer, MediaDelivery, MediaFormat, MediaKind, SourceUrl,
    VideoCodec,
};

#[test]
fn direct_choice_is_not_wholesale() {
    let c = FormatChoice::new(None, None, MediaContainer::Mp4, None);
    assert!(!c.is_wholesale());
}

#[test]
fn wholesale_choice_reports_itself_and_skips_mux() {
    let v = MediaFormat::new(
        "270",
        MediaKind::VideoOnly,
        Some(1080),
        None,
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        None,
        None,
        None,
        SourceUrl::new("https://x/m.m3u8").unwrap(),
    )
    .with_delivery(MediaDelivery::Hls);
    let a = MediaFormat::new(
        "233",
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(AudioCodec::Aac),
        MediaContainer::M4a,
        None,
        None,
        None,
        SourceUrl::new("https://x/a.m3u8").unwrap(),
    )
    .with_delivery(MediaDelivery::Hls);
    let c = FormatChoice::new(Some(v), Some(a), MediaContainer::Mp4, None)
        .with_wholesale_selector("bv*+ba/b");
    assert!(c.is_wholesale());
    assert!(!c.requires_mux());
}

#[test]
fn wholesale_selector_round_trips_through_codable() {
    let c = FormatChoice::new(None, None, MediaContainer::Webm, Some(42))
        .with_wholesale_selector("233+ba/b");
    let data = serde_json::to_vec(&c).unwrap();
    assert_eq!(serde_json::from_slice::<FormatChoice>(&data).unwrap(), c);
}
