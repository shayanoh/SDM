//! The free functions the Swift views call.

mod support;

use sdm_core::{
    AudioCodec, ByteRange, MediaContainer, MediaFormat, MediaKind, QualityPreferences,
    ResolvedMedia, VideoCodec,
};
use sdm_ffi::custom_types::RangeSetData;
use sdm_ffi::*;
use sdm_grabber::{MediaRow, MediaRowState, ProbedLink};
use support::su;

fn video(id: &str, height: i64) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::VideoOnly,
        Some(height),
        Some(height * 16 / 9),
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        Some(1000),
        None,
        Some(1000.0),
        su(&format!("https://gv/{id}")),
    )
}

fn audio(id: &str) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(AudioCodec::Aac),
        MediaContainer::M4a,
        Some(100),
        None,
        Some(128.0),
        su(&format!("https://gv/{id}")),
    )
}

fn media() -> ResolvedMedia {
    ResolvedMedia::new(
        "youtube",
        "abc123",
        "A Title",
        Some(60.0),
        vec![video("v1080", 1080), video("v720", 720), audio("a1")],
    )
}

#[test]
fn core_version_is_the_crate_version() {
    assert_eq!(core_version().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn timing_constants_match_the_swift_core() {
    assert_eq!(app_timing_ticks_per_second().unwrap(), 5);
    let window = live_window().unwrap();
    assert_eq!(window.eta_ticks, 50);
    assert_eq!(window.package_sparkline_samples, 64);
    assert_eq!(window.item_history_samples, 64);
}

#[test]
fn speed_helpers_follow_the_snapshot_aggregates() {
    assert_eq!(average_recent_speed(vec![], 7.0).unwrap(), 7.0);
    assert_eq!(average_recent_speed(vec![2.0, 4.0], 7.0).unwrap(), 3.0);
    assert_eq!(estimated_seconds_remaining(1000, 400, 100.0).unwrap(), 6.0);
    assert_eq!(estimated_seconds_remaining(1000, 400, 0.0).unwrap(), 0.0);
    assert_eq!(estimated_seconds_remaining(0, 0, 100.0).unwrap(), 0.0);
}

#[test]
fn the_theme_catalog_lists_every_built_in_theme_sorted() {
    let themes = theme_catalog().unwrap();
    assert_eq!(themes.len(), 16);
    let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn contrast_ratio_validates_its_colors() {
    assert!((contrast_ratio("#000000".into(), "#FFFFFF".into()).unwrap() - 21.0).abs() < 1e-9);
    assert!(contrast_passes_aa("#000000".into(), "#FFFFFF".into()).unwrap());
    assert!(!contrast_passes_aa("#777777".into(), "#888888".into()).unwrap());
    for bad in ["", "#12345", "#GGGGGG", "red", "#1234567"] {
        assert!(
            matches!(
                contrast_ratio(bad.into(), "#FFFFFF".into()),
                Err(CoreError::InvalidArgument { .. })
            ),
            "{bad:?} must be refused, not asserted on"
        );
    }
}

#[test]
fn the_site_registry_recognises_curated_hosts_only() {
    assert!(site_registry_can_handle(su("https://www.youtube.com/watch?v=abc")).unwrap());
    assert!(site_registry_can_handle(su("https://vimeo.com/123")).unwrap());
    assert!(!site_registry_can_handle(su("https://example.com/file.zip")).unwrap());
}

#[test]
fn the_format_selector_picks_and_ranks() {
    let preferences = QualityPreferences::default();
    let choice = format_selector_pick(media(), preferences.clone())
        .unwrap()
        .expect("a format is picked");
    assert_eq!(choice.video.as_ref().map(|f| f.id.as_str()), Some("v1080"));
    assert_eq!(choice.audio.as_ref().map(|f| f.id.as_str()), Some("a1"));

    let ranked = format_selector_ranked_video_formats(media(), preferences.clone()).unwrap();
    assert_eq!(ranked[0].id, "v1080");
    let audio_ranked = format_selector_ranked_audio_formats(media(), preferences).unwrap();
    assert_eq!(audio_ranked[0].id, "a1");
    assert!(format_selector_video_rank_less(video("a", 1080), video("b", 720)).unwrap());
    assert_eq!(
        format_selector_wholesale_selector(720).unwrap(),
        "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b"
    );
}

#[test]
fn the_format_menu_lists_matching_options_first() {
    let options = media_format_menu_options(media(), QualityPreferences::default()).unwrap();
    assert!(!options.is_empty());
    assert!(options[0].matches_preferences);
}

#[test]
fn extract_urls_finds_distinct_grabbable_links() {
    let urls = extract_urls(
        "a https://example.com/a.zip and https://example.com/a.zip and https://example.com/ b"
            .into(),
    )
    .unwrap();
    assert_eq!(urls, vec![su("https://example.com/a.zip")]);
    assert!(is_grabbable(su("https://example.com/file.zip")).unwrap());
    assert!(!is_grabbable(su("https://example.com/")).unwrap());
}

#[test]
fn media_handoff_builds_items_and_counts_held_back_rows() {
    let link = ProbedLink::new(su("https://example.com/movie.mp4")).with_content_length(2_000);
    let resolving = MediaRow::new(su("https://www.youtube.com/watch?v=abc"));
    assert_eq!(resolving.state, MediaRowState::Resolving);
    let result = media_handoff_build(vec![link], vec![resolving]).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.held_back_count, 1);
    assert_eq!(result.items[0].output_filename, "movie.mp4");
}

#[test]
fn range_set_helpers_merge_total_and_find_gaps() {
    let empty = RangeSetData { ranges: vec![] };
    let one = range_set_inserting(empty, ByteRange::new(0, 10)).unwrap();
    let two = range_set_inserting(one, ByteRange::new(20, 30)).unwrap();
    assert_eq!(range_set_total_bytes(two.clone()).unwrap(), 20);
    assert_eq!(
        range_set_gaps_within(two.clone(), 40).unwrap(),
        vec![ByteRange::new(10, 20), ByteRange::new(30, 40)]
    );
    let merged = range_set_inserting(two, ByteRange::new(8, 22)).unwrap();
    assert_eq!(merged.ranges, vec![ByteRange::new(0, 30)]);
    assert!(matches!(
        range_set_inserting(merged.clone(), ByteRange { start: 5, end: 1 }),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert!(matches!(
        range_set_gaps_within(merged, -1),
        Err(CoreError::InvalidArgument { .. })
    ));
}
