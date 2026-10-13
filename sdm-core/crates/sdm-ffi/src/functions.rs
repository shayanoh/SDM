//! The pure functions the Swift views and controllers call directly.
//!
//! Each one that could panic on a bad argument validates it first and
//! returns [`CoreError::InvalidArgument`], and each runs under `guard`, so
//! the Swift binding is always a throwing call.

use sdm_core::{
    AppTiming, ByteRange, ContrastRatio, FormatChoice, MediaFormat, QualityPreferences, RangeSet,
    ResolvedMedia, SourceUrl, Theme, ThemeCatalog,
};
use sdm_engine::LiveWindow as EngineLiveWindow;
use sdm_grabber::{
    MediaFormatMenu, MediaFormatOption, MediaHandoff, MediaRow, ProbedLink, UrlExtractor,
};
use sdm_resolve::{FormatSelector, SiteRegistry};

use crate::custom_types::{RangeSetData, range_set_from_data, range_set_to_data};
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
