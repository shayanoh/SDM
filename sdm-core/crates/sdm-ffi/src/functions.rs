//! The pure functions the Swift views and controllers call directly.
//!
//! Each one that could panic on a bad argument validates it first and
//! returns [`CoreError::InvalidArgument`], and each runs under `guard`, so
//! the Swift binding is always a throwing call.

use std::path::PathBuf;

use sdm_core::{
    AppTiming, ByteRange, ContrastRatio, DownloadItem, DownloadPackage, FormatChoice, ItemState,
    MediaContainer, MediaFormat, QualityPreferences, RangeSet, ResolvedMedia, SourceUrl, Theme,
    ThemeCatalog,
};
use sdm_engine::{EngineSettings, ItemSnapshot, LiveWindow as EngineLiveWindow, PackageAggregate};
use sdm_grabber::{
    GrabberSnapshot, MediaFormatMenu, MediaFormatOption, MediaHandoff, MediaRow, ProbedLink,
    UrlExtractor,
};
use sdm_resolve::{CookieSource, FormatSelector, SiteRegistry, YtDlpChannel};

use crate::custom_types::{RangeSetData, range_set_from_data, range_set_to_data};
use crate::engine::validate_settings;
use crate::error::CoreError;
use crate::runtime::guard;

/// The version of the Rust core, for the app's startup log line.
#[uniffi::export]
pub fn core_version() -> Result<String, CoreError> {
    guard(|| env!("CARGO_PKG_VERSION").to_owned())
}

/// `AppTiming.ticksPerSecond`: the engine heartbeat rate.
#[uniffi::export]
pub fn app_timing_ticks_per_second() -> Result<usize, CoreError> {
    guard(|| AppTiming::TICKS_PER_SECOND)
}

/// `LiveWindow`'s sample-window sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LiveWindow {
    pub eta_ticks: usize,
    pub package_sparkline_samples: usize,
    pub item_history_samples: usize,
}

#[uniffi::export]
pub fn live_window() -> Result<LiveWindow, CoreError> {
    guard(|| LiveWindow {
        eta_ticks: EngineLiveWindow::ETA_TICKS,
        package_sparkline_samples: EngineLiveWindow::PACKAGE_SPARKLINE_SAMPLES,
        item_history_samples: EngineLiveWindow::ITEM_HISTORY_SAMPLES,
    })
}

/// The summed figures for `items`, as the engine computes a package's: used
/// for the Completed tab, whose package is a filtered copy of the engine's.
/// `global_recent_speed` is the global rate averaged over the ETA window.
#[uniffi::export]
pub fn package_aggregate_for_items(
    items: Vec<ItemSnapshot>,
    global_recent_speed: f64,
) -> Result<PackageAggregate, CoreError> {
    guard(|| PackageAggregate::new(&items, global_recent_speed))
}

/// `ItemSnapshot.isRetrying`: a `Queued` item that has already failed at
/// least once.
#[uniffi::export]
pub fn item_snapshot_is_retrying(item: ItemSnapshot) -> Result<bool, CoreError> {
    guard(|| item.is_retrying())
}

/// `ItemSnapshot.fractionCompleted`: 0 while the size is unknown or zero.
#[uniffi::export]
pub fn item_snapshot_fraction_completed(item: ItemSnapshot) -> Result<f64, CoreError> {
    guard(|| item.fraction_completed())
}

/// Mean of the trailing ETA-window samples of `history`, or `fallback` when
/// there is no history.
#[uniffi::export]
pub fn average_recent_speed(history: Vec<f64>, fallback: f64) -> Result<f64, CoreError> {
    guard(|| sdm_engine::average_recent_speed(&history, fallback))
}

/// Seconds left at `speed`, or 0 when it cannot be estimated.
#[uniffi::export]
pub fn estimated_seconds_remaining(
    total_bytes: i64,
    completed_bytes: i64,
    speed: f64,
) -> Result<f64, CoreError> {
    guard(|| sdm_engine::estimated_seconds_remaining(total_bytes, completed_bytes, speed))
}

// MARK: URL identity

/// A [`SourceUrl`] holding exactly `text`. The string is the link's identity
/// in the core (equality, hashing, persistence), so this is the one way to ask
/// what the core holds for a given string.
#[uniffi::export]
pub fn source_url_parse(text: String) -> Result<SourceUrl, CoreError> {
    guard(|| crate::custom_types::source_url_from_string(&text))?
}

/// The identity string the core holds for `url`: what arrived from Swift,
/// byte for byte.
#[uniffi::export]
pub fn source_url_text(url: SourceUrl) -> Result<String, CoreError> {
    guard(|| url.as_str().to_owned())
}

// MARK: Defaults and raw values

/// `QualityPreferences.default`: up to 1080p, every known codec and container.
#[uniffi::export]
pub fn quality_preferences_default() -> Result<QualityPreferences, CoreError> {
    guard(QualityPreferences::default)
}

/// `EngineSettings` with the core's defaults for everything the app does not
/// expose (per-host limit, checkpoint interval, persist debounce, minimum
/// segment size, file-check interval). The four arguments are validated like
/// `EngineSettings::new`'s, as errors rather than a panic.
#[uniffi::export]
pub fn engine_settings_defaults(
    max_concurrent: usize,
    segments_per_item: usize,
    global_max_connections: usize,
    download_folder: PathBuf,
) -> Result<EngineSettings, CoreError> {
    for (name, value) in [
        ("maxConcurrent", max_concurrent),
        ("segmentsPerItem", segments_per_item),
        ("globalMaxConnections", global_max_connections),
    ] {
        if value < 1 {
            return Err(CoreError::invalid(name, "must be at least 1"));
        }
    }
    let settings = guard(|| {
        EngineSettings::new(
            max_concurrent,
            segments_per_item,
            global_max_connections,
            download_folder,
        )
    })?;
    validate_settings(&settings)?;
    Ok(settings)
}

/// The file extension of `container` (`mp4`, `webm`, `m4a`, or the payload of
/// `Other`).
#[uniffi::export]
pub fn media_container_file_extension(container: MediaContainer) -> Result<String, CoreError> {
    guard(|| container.file_extension().to_owned())
}

/// Every [`CookieSource`], in declaration order.
#[uniffi::export]
pub fn cookie_source_all() -> Result<Vec<CookieSource>, CoreError> {
    guard(|| CookieSource::ALL.to_vec())
}

/// The string the app persists `source` as (the Swift core's `String` raw
/// value).
#[uniffi::export]
pub fn cookie_source_raw_value(source: CookieSource) -> Result<String, CoreError> {
    guard(|| source.raw_value().to_owned())
}

/// The case persisted as `raw`, if there is one.
#[uniffi::export]
pub fn cookie_source_from_raw_value(raw: String) -> Result<Option<CookieSource>, CoreError> {
    guard(|| {
        CookieSource::ALL
            .into_iter()
            .find(|source| source.raw_value() == raw)
    })
}

/// A channel's persisted string: its serde name, which is also what the
/// managed-binaries manifest stores and what the Swift core's `String` raw
/// value was.
fn channel_raw_value(channel: YtDlpChannel) -> String {
    match serde_json::to_value(channel) {
        Ok(serde_json::Value::String(raw)) => raw,
        other => panic!("a YtDlpChannel serializes as a string, got {other:?}"),
    }
}

/// Every [`YtDlpChannel`], in declaration order.
#[uniffi::export]
pub fn yt_dlp_channel_all() -> Result<Vec<YtDlpChannel>, CoreError> {
    guard(|| YtDlpChannel::ALL.to_vec())
}

/// The string the app persists `channel` as.
#[uniffi::export]
pub fn yt_dlp_channel_raw_value(channel: YtDlpChannel) -> Result<String, CoreError> {
    guard(|| channel_raw_value(channel))
}

/// The channel persisted as `raw`, if there is one.
#[uniffi::export]
pub fn yt_dlp_channel_from_raw_value(raw: String) -> Result<Option<YtDlpChannel>, CoreError> {
    guard(|| {
        YtDlpChannel::ALL
            .into_iter()
            .find(|channel| channel_raw_value(*channel) == raw)
    })
}

// MARK: Themes

/// The built-in themes, sorted by name.
#[uniffi::export]
pub fn theme_catalog() -> Result<Vec<Theme>, CoreError> {
    guard(ThemeCatalog::built_in_themes)
}

/// Whether `text` is a `#RRGGBB` (or `RRGGBB`) color, the only form
/// `ContrastRatio` accepts without asserting.
fn validate_hex(argument: &str, text: &str) -> Result<(), CoreError> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.chars().count() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CoreError::invalid(
            argument,
            format!("expected a #RRGGBB hex color, got `{text}`"),
        ))
    }
}

/// The WCAG contrast ratio between two `#RRGGBB` colors.
#[uniffi::export]
pub fn contrast_ratio(hex_a: String, hex_b: String) -> Result<f64, CoreError> {
    validate_hex("hexA", &hex_a)?;
    validate_hex("hexB", &hex_b)?;
    guard(|| ContrastRatio::between(&hex_a, &hex_b))
}

/// Whether two `#RRGGBB` colors reach the WCAG AA text contrast of 4.5.
#[uniffi::export]
pub fn contrast_passes_aa(hex_a: String, hex_b: String) -> Result<bool, CoreError> {
    validate_hex("hexA", &hex_a)?;
    validate_hex("hexB", &hex_b)?;
    guard(|| ContrastRatio::passes_aa(&hex_a, &hex_b))
}

// MARK: Resolver

/// Whether the curated site registry has a pattern for `url`'s host, i.e.
/// whether SDM resolves it with yt-dlp.
#[uniffi::export]
pub fn site_registry_can_handle(url: SourceUrl) -> Result<bool, CoreError> {
    guard(|| {
        url.as_url()
            .is_some_and(|parsed| SiteRegistry::match_url(parsed).is_some())
    })
}

/// The format SDM would pick for `media` under `preferences`, if any.
#[uniffi::export]
pub fn format_selector_pick(
    media: ResolvedMedia,
    preferences: QualityPreferences,
) -> Result<Option<FormatChoice>, CoreError> {
    guard(|| FormatSelector::pick(&media, &preferences))
}

/// The yt-dlp `-f` expression a wholesale download uses.
#[uniffi::export]
pub fn format_selector_wholesale_selector(max_height: i64) -> Result<String, CoreError> {
    guard(|| FormatSelector::wholesale_selector(max_height))
}

#[uniffi::export]
pub fn format_selector_ranked_video_formats(
    media: ResolvedMedia,
    preferences: QualityPreferences,
) -> Result<Vec<MediaFormat>, CoreError> {
    guard(|| FormatSelector::ranked_video_formats(&media, &preferences))
}

#[uniffi::export]
pub fn format_selector_ranked_audio_formats(
    media: ResolvedMedia,
    preferences: QualityPreferences,
) -> Result<Vec<MediaFormat>, CoreError> {
    guard(|| FormatSelector::ranked_audio_formats(&media, &preferences))
}

/// Whether `a` ranks before `b` in the picker.
#[uniffi::export]
pub fn format_selector_video_rank_less(a: MediaFormat, b: MediaFormat) -> Result<bool, CoreError> {
    guard(|| FormatSelector::video_rank_less(&a, &b))
}

// MARK: Grabber

/// The flat, match-first format-picker list for a media row.
#[uniffi::export]
pub fn media_format_menu_options(
    media: ResolvedMedia,
    preferences: QualityPreferences,
) -> Result<Vec<MediaFormatOption>, CoreError> {
    guard(|| MediaFormatMenu::options(&media, &preferences))
}

/// Distinct grabbable links in `text`, in order of first appearance.
#[uniffi::export]
pub fn extract_urls(text: String) -> Result<Vec<SourceUrl>, CoreError> {
    guard(|| UrlExtractor::extract_links(&text))
}

#[uniffi::export]
pub fn is_grabbable(url: SourceUrl) -> Result<bool, CoreError> {
    guard(|| UrlExtractor::is_grabbable(&url))
}

/// What `media_handoff_build` produced.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HandoffResult {
    pub items: Vec<sdm_core::DownloadItem>,
    /// Media rows held back because no format was resolved yet.
    pub held_back_count: usize,
}

/// Translates a package's grabber rows into engine `DownloadItem`s.
#[uniffi::export]
pub fn media_handoff_build(
    http_links: Vec<ProbedLink>,
    media_rows: Vec<MediaRow>,
) -> Result<HandoffResult, CoreError> {
    guard(|| {
        let (items, held_back_count) = MediaHandoff::build(&http_links, &media_rows);
        HandoffResult {
            items,
            held_back_count,
        }
    })
}

// MARK: Byte ranges

/// `set` with `range` added, merged with whatever it touches.
#[uniffi::export]
pub fn range_set_inserting(set: RangeSetData, range: ByteRange) -> Result<RangeSetData, CoreError> {
    if range.start < 0 || range.end < range.start {
        return Err(CoreError::invalid(
            "range",
            format!("invalid byte range {}..{}", range.start, range.end),
        ));
    }
    let mut rebuilt = range_set_from_data(&set)?;
    guard(|| {
        rebuilt.insert(ByteRange::new(range.start, range.end));
        range_set_to_data(&rebuilt)
    })
}

/// One selected item's completed ranges and where its bytes start in the
/// combined space.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OffsetRangeSet {
    pub set: RangeSetData,
    pub offset: i64,
}

/// The union of every part's ranges shifted by its offset, merged: the
/// combined progress bar of several items laid end to end (each item's
/// offset is the total size of the items before it).
#[uniffi::export]
pub fn range_set_union(parts: Vec<OffsetRangeSet>) -> Result<RangeSetData, CoreError> {
    let mut combined = RangeSet::new();
    for part in &parts {
        if part.offset < 0 {
            return Err(CoreError::invalid("offset", "must not be negative"));
        }
        for range in range_set_from_data(&part.set)?.ranges() {
            let (Some(start), Some(end)) = (
                range.start.checked_add(part.offset),
                range.end.checked_add(part.offset),
            ) else {
                return Err(CoreError::invalid("offset", "shifts a range out of bounds"));
            };
            combined.insert(ByteRange::new(start, end));
        }
    }
    guard(|| range_set_to_data(&combined))
}

/// How many bytes `set` covers.
#[uniffi::export]
pub fn range_set_total_bytes(set: RangeSetData) -> Result<i64, CoreError> {
    let rebuilt: RangeSet = range_set_from_data(&set)?;
    guard(|| rebuilt.total_bytes())
}

/// The gaps of `set` inside `0..total`.
#[uniffi::export]
pub fn range_set_gaps_within(set: RangeSetData, total: i64) -> Result<Vec<ByteRange>, CoreError> {
    if total < 0 {
        return Err(CoreError::invalid("total", "must not be negative"));
    }
    let rebuilt = range_set_from_data(&set)?;
    guard(|| rebuilt.gaps_within(total))
}

/// The verdict counts the link grabber's header and chips show, computed once
/// per snapshot (the Swift `GrabberSnapshot` computed properties).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct GrabberSummary {
    pub online_count: usize,
    pub offline_count: usize,
    pub faulty_count: usize,
    pub failed_count: usize,
    pub is_checking: bool,
    pub recheckable_count: usize,
}

#[uniffi::export]
pub fn grabber_snapshot_summary(snapshot: GrabberSnapshot) -> Result<GrabberSummary, CoreError> {
    guard(|| GrabberSummary {
        online_count: snapshot.online_count(),
        offline_count: snapshot.offline_count(),
        faulty_count: snapshot.faulty_count(),
        failed_count: snapshot.failed_count(),
        is_checking: snapshot.is_checking(),
        recheckable_count: snapshot.recheckable_count(),
    })
}

/// The name used for clustering and display of a probed link.
#[uniffi::export]
pub fn probed_link_effective_filename(link: ProbedLink) -> Result<String, CoreError> {
    guard(|| link.effective_filename())
}

/// `"<sanitized title> [<videoID>].<container>"` once a media row is resolved,
/// the URL-derived title before that.
#[uniffi::export]
pub fn media_row_display_filename(row: MediaRow) -> Result<String, CoreError> {
    guard(|| row.display_filename())
}

// MARK: Constructors

/// A one-component HTTP item for `url` with a fresh id, enabled, no priority.
/// `total_bytes` is the size when known.
#[uniffi::export]
pub fn download_item_for_url(
    url: SourceUrl,
    filename: String,
    total_bytes: Option<i64>,
    state: ItemState,
) -> Result<DownloadItem, CoreError> {
    if filename.is_empty() {
        return Err(CoreError::invalid("filename", "must not be empty"));
    }
    if total_bytes.is_some_and(|bytes| bytes < 0) {
        return Err(CoreError::invalid("totalBytes", "must not be negative"));
    }
    guard(|| {
        let item = DownloadItem::http(url, filename).with_state(state);
        match total_bytes {
            Some(bytes) => item.with_total_bytes(bytes),
            None => item,
        }
    })
}

/// A package with a fresh id, normal priority and position 0.
#[uniffi::export]
pub fn download_package_new(
    name: String,
    items: Vec<DownloadItem>,
    note: Option<String>,
) -> Result<DownloadPackage, CoreError> {
    if name.is_empty() {
        return Err(CoreError::invalid("name", "must not be empty"));
    }
    guard(|| {
        let package = DownloadPackage::new(name).with_items(items);
        match note {
            Some(note) => package.with_note(note),
            None => package,
        }
    })
}
