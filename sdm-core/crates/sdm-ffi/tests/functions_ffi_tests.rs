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

#[test]
fn constructors_fill_ids_and_defaults() {
    let item = download_item_for_url(
        su("https://example.com/a.bin"),
        "a.bin".to_owned(),
        Some(10),
        sdm_core::ItemState::Stopped,
    )
    .unwrap();
    assert_eq!(item.output_filename, "a.bin");
    assert_eq!(item.state, sdm_core::ItemState::Stopped);
    assert!(item.is_enabled);
    assert_eq!(item.total_bytes(), Some(10));
    let other = download_item_for_url(
        su("https://example.com/a.bin"),
        "a.bin".to_owned(),
        None,
        sdm_core::ItemState::Queued,
    )
    .unwrap();
    assert_ne!(item.id, other.id);
    assert_eq!(other.total_bytes(), None);
    assert!(matches!(
        download_item_for_url(
            su("https://example.com/a"),
            String::new(),
            None,
            sdm_core::ItemState::Queued
        ),
        Err(CoreError::InvalidArgument { .. })
    ));

    let package =
        download_package_new("Pack".to_owned(), vec![item], Some("note".to_owned())).unwrap();
    assert_eq!(package.name, "Pack");
    assert_eq!(package.items.len(), 1);
    assert_eq!(package.note.as_deref(), Some("note"));
    assert!(matches!(
        download_package_new(String::new(), vec![], None),
        Err(CoreError::InvalidArgument { .. })
    ));
}

#[test]
fn grabber_summary_and_names_match_the_core_methods() {
    let link = ProbedLink::new(su("https://example.com/movie.mp4"));
    assert_eq!(
        probed_link_effective_filename(link.clone()).unwrap(),
        "movie.mp4"
    );
    let row = MediaRow::new(su("https://www.youtube.com/watch?v=abc"));
    assert_eq!(
        media_row_display_filename(row.clone()).unwrap(),
        row.display_filename()
    );
    let snapshot = sdm_grabber::GrabberSnapshot::new(vec![link], vec![row], vec![], 0, 2);
    let summary = grabber_snapshot_summary(snapshot.clone()).unwrap();
    assert_eq!(summary.online_count, snapshot.online_count());
    assert_eq!(summary.recheckable_count, snapshot.recheckable_count());
    assert!(summary.is_checking);
}

#[test]
fn package_aggregate_for_items_matches_the_engine_computation() {
    let empty = package_aggregate_for_items(vec![], 0.0).unwrap();
    assert_eq!(empty, sdm_engine::PackageAggregate::new(&[], 0.0));
    assert_eq!(empty.total_bytes, 0);
}

fn finished_item(total: i64, bytes_per_second: f64) -> sdm_engine::ItemSnapshot {
    let mut completed = sdm_core::RangeSet::new();
    completed.insert(ByteRange::new(0, total));
    sdm_engine::ItemSnapshot::new(
        uuid::Uuid::new_v4(),
        su("https://example.com/a.bin"),
        "a.bin",
        Some(total),
        completed,
        sdm_core::ItemState::Completed,
        true,
        Some(true),
        0,
        1,
        bytes_per_second,
        vec![],
    )
}

#[test]
fn package_aggregate_for_items_sums_the_items_like_the_engine() {
    let items = vec![finished_item(100, 5.0), finished_item(300, 7.0)];
    let aggregate = package_aggregate_for_items(items.clone(), 12.0).unwrap();
    assert_eq!(aggregate, sdm_engine::PackageAggregate::new(&items, 12.0));
    assert_eq!(aggregate.total_bytes, 400);
    assert_eq!(aggregate.completed_bytes, 400);
}

#[test]
fn item_snapshot_helpers_forward_to_the_engine_rules() {
    let mut item = finished_item(200, 0.0);
    item.completed = sdm_core::RangeSet::new();
    item.completed.insert(ByteRange::new(0, 50));
    assert_eq!(
        item_snapshot_fraction_completed(item.clone()).unwrap(),
        0.25
    );
    item.total_bytes = None;
    assert_eq!(item_snapshot_fraction_completed(item.clone()).unwrap(), 0.0);

    item.state = sdm_core::ItemState::Queued;
    assert!(!item_snapshot_is_retrying(item.clone()).unwrap());
    item.failed_attempt_count = Some(1);
    assert!(item_snapshot_is_retrying(item.clone()).unwrap());
    item.state = sdm_core::ItemState::Running;
    assert!(!item_snapshot_is_retrying(item).unwrap());
}

#[test]
fn container_extensions_come_from_the_core() {
    assert_eq!(
        media_container_file_extension(MediaContainer::Mp4).unwrap(),
        "mp4"
    );
    assert_eq!(
        media_container_file_extension(MediaContainer::M4a).unwrap(),
        "m4a"
    );
    assert_eq!(
        media_container_file_extension(MediaContainer::Other("ogg".into())).unwrap(),
        "ogg"
    );
}

/// UserDefaults already holds these strings; they must never change.
#[test]
fn cookie_source_raw_values_are_the_persisted_strings() {
    use sdm_resolve::CookieSource::*;
    let expected = [
        (None, "none"),
        (Safari, "safari"),
        (Chrome, "chrome"),
        (Firefox, "firefox"),
        (Edge, "edge"),
        (Brave, "brave"),
    ];
    assert_eq!(
        cookie_source_all().unwrap(),
        expected.iter().map(|(s, _)| *s).collect::<Vec<_>>()
    );
    for (source, raw) in expected {
        assert_eq!(cookie_source_raw_value(source).unwrap(), raw);
        assert_eq!(
            cookie_source_from_raw_value(raw.to_owned()).unwrap(),
            Some(source)
        );
    }
    for bad in ["", "Safari", "bogus"] {
        assert_eq!(
            cookie_source_from_raw_value(bad.to_owned()).unwrap(),
            Option::None
        );
    }
}

#[test]
fn channel_raw_values_are_the_persisted_strings() {
    use sdm_resolve::YtDlpChannel::*;
    assert_eq!(yt_dlp_channel_all().unwrap(), vec![Stable, Nightly]);
    for (channel, raw) in [(Stable, "stable"), (Nightly, "nightly")] {
        assert_eq!(yt_dlp_channel_raw_value(channel).unwrap(), raw);
        assert_eq!(
            yt_dlp_channel_from_raw_value(raw.to_owned()).unwrap(),
            Some(channel)
        );
    }
    assert_eq!(yt_dlp_channel_from_raw_value(String::new()).unwrap(), None);
}

#[test]
fn quality_preferences_default_is_the_core_default() {
    assert_eq!(
        quality_preferences_default().unwrap(),
        QualityPreferences::default()
    );
    assert_eq!(quality_preferences_default().unwrap().max_height, 1080);
}

#[test]
fn engine_settings_defaults_match_the_core_and_validate() {
    let folder = std::path::PathBuf::from("/tmp/x");
    let settings = engine_settings_defaults(2, 4, 16, folder.clone()).unwrap();
    assert_eq!(
        settings,
        sdm_engine::EngineSettings::new(2, 4, 16, folder.clone())
    );
    assert_eq!(settings.max_connections_per_host, 8);
    assert_eq!(settings.checkpoint_interval_bytes, 8 * 1024 * 1024);
    assert_eq!(settings.min_segment_size_bytes, 10 * 1024 * 1024);
    for (a, b, c) in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
        assert!(matches!(
            engine_settings_defaults(a, b, c, folder.clone()),
            Err(CoreError::InvalidArgument { .. })
        ));
    }
}

#[test]
fn range_set_union_shifts_and_merges() {
    let set = |ranges: &[(i64, i64)]| RangeSetData {
        ranges: ranges.iter().map(|(s, e)| ByteRange::new(*s, *e)).collect(),
    };
    // Item A: 100 bytes, all done. Item B starts at 100, first 50 done and 80..100.
    let merged = range_set_union(vec![
        OffsetRangeSet {
            set: set(&[(0, 100)]),
            offset: 0,
        },
        OffsetRangeSet {
            set: set(&[(0, 50), (80, 100)]),
            offset: 100,
        },
    ])
    .unwrap();
    assert_eq!(merged, set(&[(0, 150), (180, 200)]));
    assert_eq!(range_set_union(vec![]).unwrap(), set(&[]));
    assert!(matches!(
        range_set_union(vec![OffsetRangeSet {
            set: set(&[(0, 1)]),
            offset: -1
        }]),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert!(matches!(
        range_set_union(vec![OffsetRangeSet {
            set: set(&[(0, 1)]),
            offset: i64::MAX
        }]),
        Err(CoreError::InvalidArgument { .. })
    ));
}

#[test]
fn priority_raw_values_are_pinned() {
    use sdm_core::Priority::*;
    for (priority, raw) in [
        (Lowest, 0i64),
        (Low, 1),
        (Normal, 2),
        (High, 3),
        (Highest, 4),
    ] {
        assert_eq!(priority as i64, raw);
    }
}

#[test]
fn source_url_text_round_trips_and_refuses_the_placeholder() {
    let url = source_url_parse("https://EXAMPLE.com/a b".to_owned()).unwrap();
    assert_eq!(source_url_text(url).unwrap(), "https://EXAMPLE.com/a b");
    assert!(matches!(
        source_url_parse("about:invalid".to_owned()),
        Err(CoreError::InvalidArgument { .. })
    ));
}
