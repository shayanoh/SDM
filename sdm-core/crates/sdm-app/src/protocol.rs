//! The wire types. Everything the UI receives or sends is defined here, so one
//! file shows the whole protocol. Ids are lowercase hyphenated UUID strings;
//! byte counts are `f64` (exact to 2^53, and `specta` rejects 64-bit integers).

use serde::{Deserialize, Serialize};
use specta::Type;

/// One ordered update. The receiver applies patches in `seq` order; a patch
/// with `full` set replaces all prior state.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    /// `+1` per emitted patch, wrapping. Any non-consecutive value is a gap.
    pub seq: u32,
    pub full: bool,
    /// Present only when the shape of the list (or a row's non-numeric content) changed.
    pub structure: Option<Structure>,
    pub items: Vec<ItemFigures>,
    pub packages: Vec<PackageFigures>,
    pub stats: Option<EngineStats>,
    /// Full speed history, only for items whose details panel is open.
    pub histories: Vec<ItemHistory>,
    /// The whole link-grabber view, present when it changed (and on a full patch).
    pub grabber: Option<GrabberView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    pub packages: Vec<StructurePackage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StructurePackage {
    pub id: String,
    pub name: String,
    /// `Priority` as 0 (lowest) to 4 (highest).
    pub priority: u32,
    /// The absolute destination folder: the download folder joined with the
    /// package name.
    pub folder: String,
    pub items: Vec<StructureItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StructureItem {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub state: ItemStateDto,
    pub is_enabled: bool,
    pub is_resumable: Option<bool>,
    pub is_assembling: bool,
    /// What happens once every component has downloaded.
    pub assembly: AssemblyDto,
    pub file_missing: bool,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AssemblyDto {
    None,
    Mux,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ItemStateDto {
    Stopped,
    Queued,
    Running,
    Completed,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RangeDto {
    pub start: f64,
    pub end: f64,
}

/// The numbers that change while an item transfers.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemFigures {
    pub id: String,
    pub completed: Vec<RangeDto>,
    pub completed_bytes: f64,
    pub total_bytes: Option<f64>,
    pub active_segments: u32,
    pub configured_segments: u32,
    pub bytes_per_second: f64,
    /// The short trailing history every snapshot carries (64 samples).
    pub speed_history: Vec<f64>,
    pub retry_hold_seconds: Option<u32>,
    pub failed_attempt_count: Option<u32>,
    pub remaining_attempts: Option<u32>,
    pub last_failure_reason: Option<String>,
    pub checkpoint_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageFigures {
    pub id: String,
    pub completed_bytes: f64,
    pub total_bytes: f64,
    pub bytes_per_second: f64,
    pub speed_history: Vec<f64>,
    pub eta_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    pub bytes_per_second: f64,
    pub history: Vec<f64>,
    pub running_count: u32,
    pub queued_count: u32,
    pub overall_fraction: f64,
    pub ring_completed_bytes: f64,
    pub ring_total_bytes: f64,
    pub waiting_item_count: u32,
    pub waiting_package_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemHistory {
    pub id: String,
    pub speed_history: Vec<f64>,
}

/// How long an ungrabbed link or media row stays in the link grabber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AutoClearInterval {
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    OneDay,
    Never,
}

impl AutoClearInterval {
    /// `None` for `Never`.
    pub fn duration(self) -> Option<std::time::Duration> {
        let seconds = match self {
            Self::FifteenMinutes => 15 * 60,
            Self::ThirtyMinutes => 30 * 60,
            Self::OneHour => 60 * 60,
            Self::OneDay => 24 * 60 * 60,
            Self::Never => return None,
        };
        Some(std::time::Duration::from_secs(seconds))
    }
}

/// Where the app shows itself. Only macOS offers a choice; the other
/// platforms always store `Both`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode {
    MenuBarOnly,
    DockOnly,
    Both,
}

/// Every setting the Settings window edits, as stored in `settings.json` and
/// sent over the wire. The defaults are the SwiftUI app's. `download_folder`
/// is always `Some` when read; on update, `None` keeps the stored folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub download_folder: Option<String>,

    pub max_concurrent: u32,
    pub segments_per_item: u32,
    pub global_max_connections: u32,
    pub max_connections_per_host: u32,
    pub min_segment_size_mb: u32,
    pub auto_start_downloads_on_launch: bool,

    pub clipboard_watching_enabled: bool,
    pub auto_add_and_start_on_grab: bool,
    pub deep_sniff_enabled: bool,
    pub auto_clear_interval: AutoClearInterval,

    pub yt_max_height: u32,
    pub allow_av1: bool,
    pub allow_vp9: bool,
    pub allow_h264: bool,
    pub allow_mp4: bool,
    pub allow_webm: bool,
    pub allow_opus: bool,
    pub allow_aac: bool,
    pub max_playlist_videos: u32,
    /// `none`, `safari`, `chrome`, `firefox`, `edge` or `brave`.
    pub cookie_source: String,
    /// `stable` or `nightly`.
    pub yt_dlp_channel: String,

    pub notify_download_finished: bool,
    pub notify_package_finished: bool,
    pub notify_download_failed: bool,
    pub notify_links_grabbed: bool,

    pub launch_at_login: bool,
    pub activation_mode: ActivationMode,
    /// A bundled theme's id, or `system` to follow the OS.
    pub theme_id: String,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            download_folder: None,
            max_concurrent: 3,
            segments_per_item: 8,
            global_max_connections: 32,
            max_connections_per_host: 8,
            min_segment_size_mb: 10,
            auto_start_downloads_on_launch: false,
            clipboard_watching_enabled: true,
            auto_add_and_start_on_grab: false,
            deep_sniff_enabled: true,
            auto_clear_interval: AutoClearInterval::ThirtyMinutes,
            yt_max_height: 1080,
            allow_av1: true,
            allow_vp9: true,
            allow_h264: true,
            allow_mp4: true,
            allow_webm: true,
            allow_opus: true,
            allow_aac: true,
            max_playlist_videos: 50,
            cookie_source: "none".to_owned(),
            yt_dlp_channel: "stable".to_owned(),
            notify_download_finished: true,
            notify_package_finished: true,
            notify_download_failed: true,
            notify_links_grabbed: true,
            launch_at_login: false,
            activation_mode: ActivationMode::Both,
            theme_id: "system".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinariesStatusDto {
    /// The tools ship inside the app (macOS) rather than being downloaded.
    pub bundled: bool,
    /// A tool is being downloaded right now.
    pub downloading: bool,
    pub yt_dlp_version: Option<String>,
    pub latest_known: Option<String>,
    pub ffmpeg_version: Option<String>,
    pub qjs_version: Option<String>,
    pub last_error: Option<String>,
}

/// A bundled theme, for the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
}

/// What a "Remove and Delete" confirmation is about.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeletionTarget {
    Items { ids: Vec<String> },
    Package { id: String },
}

/// The files a deletion would trash, for the confirmation sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeletionInfo {
    pub title: String,
    pub total_bytes: f64,
    pub names: Vec<String>,
}

// MARK: Link grabber

/// Everything the link grabber shows. Sent whole whenever it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GrabberView {
    pub checked_count: u32,
    pub total_count: u32,
    pub summary: GrabberSummaryDto,
    /// Links and media rows in session order.
    pub rows: Vec<GrabberRow>,
    pub packages: Vec<GrabberPackage>,
    /// How many items the last hand-off held back (no format chosen yet).
    pub held_back: Option<u32>,
    pub tools: Option<ToolsNotice>,
}

impl GrabberView {
    pub fn empty() -> Self {
        Self {
            checked_count: 0,
            total_count: 0,
            summary: GrabberSummaryDto::default(),
            rows: Vec::new(),
            packages: Vec::new(),
            held_back: None,
            tools: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GrabberSummaryDto {
    pub online: u32,
    pub faulty: u32,
    pub offline: u32,
    pub failed: u32,
    pub is_checking: bool,
    pub recheckable: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GrabberPackage {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    /// Links first, then media rows.
    pub row_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GrabberRow {
    Link(GrabberLinkRow),
    Media(GrabberMediaRow),
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GrabberLinkRow {
    pub id: String,
    pub filename: String,
    pub final_url: String,
    pub stage: LinkStageDto,
    pub verdict: Option<VerdictDto>,
    pub size_bytes: Option<f64>,
    pub is_duplicate: bool,
    /// Whether the trash button shows (a verdict exists).
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GrabberMediaRow {
    pub id: String,
    pub title: String,
    pub source_url: String,
    pub state: MediaStateDto,
    /// The full failure reason when `state` is `failed`.
    pub state_text: Option<String>,
    /// `Streaming · 1080p · avc1 · mp4`, once a format is chosen.
    pub summary: Option<String>,
    pub options: Vec<FormatOptionDto>,
    pub is_duplicate: bool,
    pub size_bytes: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LinkStageDto {
    Queued,
    Probing,
    Sniffing,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VerdictDto {
    Online,
    Offline,
    CheckFailed,
    Faulty { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MediaStateDto {
    Resolving,
    Resolved,
    Unselected,
    Unsupported,
    NeedsYtDlp,
    NeedsFfmpeg,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FormatOptionDto {
    pub id: String,
    pub label: String,
    pub matches_preferences: bool,
    pub is_wholesale: bool,
    pub selected: bool,
}

/// A line under the grabber header about the downloaded tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolsNotice {
    pub text: String,
    pub is_error: bool,
}

/// What an "Add to downloads" acts on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HandoffTarget {
    Rows { ids: Vec<String> },
    Package { id: String },
    All,
}
