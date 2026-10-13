//! UniFFI declarations for the domain types of the four library crates.
//!
//! The library crates stay free of UniFFI: each type is declared here with
//! `#[uniffi::remote]`, mirroring the real definition field for field. The
//! mirror is not compiled as a type of its own; UniFFI generates the
//! converters for the real one, and a field that drifts from the real
//! definition fails to compile here. Generated Swift types carry the same
//! names as the Rust ones (and as the Swift core's), so the app's source
//! keeps reading `DownloadItem`, `ItemSnapshot` and so on.
//!
//! Declaring them in this one crate also means one UniFFI scaffolding, one
//! `uniffi.toml` and one generated Swift file, instead of one per library
//! crate (each of which needs its own `uniffi.toml` to keep the generated
//! modules from colliding).
//!
//! `usize` fields cross as Swift `Int` (see `custom_types`).

use std::collections::HashSet;
use std::path::PathBuf;

use sdm_core::{
    Assembly, AudioCodec, ByteRange, ComponentError, ComponentOrigin, DownloadItem,
    DownloadPackage, FileComponent, FormatChoice, ItemState, MediaContainer, MediaDelivery,
    MediaFormat, MediaKind, Priority, QualityPreferences, RangeSet, ResolvedMedia, SourceUrl,
    Theme, VideoCodec,
};
use sdm_engine::{
    EngineFigures, EngineSettings, EngineSnapshot, ItemSnapshot, PackageAggregate, PackageSnapshot,
};
use sdm_grabber::{
    FileSignature, GrabberBudget, GrabberSnapshot, MediaFormatOption, MediaRow, MediaRowState,
    PackageCandidate, ProbedLink, Stage, Verdict,
};
use sdm_resolve::{
    CheckOutcome, CheckReason, CookieSource, ManagedBinariesStatus, VendorAsset, YtDlpChannel,
};
use uuid::Uuid;

// MARK: sdm-core

#[uniffi::remote(Record)]
pub struct ByteRange {
    pub start: i64,
    pub end: i64,
}

// `repr(i64)` and the explicit values mirror the real type; the compiler does
// not compare them (only UniFFI's metadata sees them), so
// `tests/functions_ffi_tests.rs` pins each variant's value.
#[uniffi::remote(Enum)]
#[repr(i64)]
pub enum Priority {
    Lowest = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Highest = 4,
}

#[uniffi::remote(Enum)]
pub enum ItemState {
    Stopped,
    Queued,
    Running,
    Completed,
    Failed { reason: String },
}

#[uniffi::remote(Enum)]
pub enum Assembly {
    None,
    Mux,
}

#[uniffi::remote(Enum)]
pub enum ComponentOrigin {
    Http,
    Resolved { format_id: String },
    Wholesale { format_selector: String },
}

#[uniffi::remote(Record)]
pub struct ComponentError {
    pub message: String,
}

#[uniffi::remote(Record)]
pub struct FileComponent {
    pub id: Uuid,
    pub url: SourceUrl,
    pub part_filename: String,
    pub total_bytes: Option<i64>,
    pub completed: RangeSet,
    pub validator: Option<String>,
    pub origin: ComponentOrigin,
    pub is_resumable: Option<bool>,
    pub last_error: Option<ComponentError>,
}

#[uniffi::remote(Record)]
pub struct DownloadItem {
    pub id: Uuid,
    pub components: Vec<FileComponent>,
    pub output_filename: String,
    pub source_url: SourceUrl,
    pub assembly: Assembly,
    pub state: ItemState,
    pub is_enabled: bool,
    pub priority: Option<Priority>,
    pub position: i64,
    pub metadata: Option<String>,
}

#[uniffi::remote(Record)]
pub struct DownloadPackage {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<DownloadItem>,
    pub priority: Priority,
    pub position: i64,
    pub note: Option<String>,
}

#[uniffi::remote(Enum)]
pub enum MediaKind {
    Progressive,
    VideoOnly,
    AudioOnly,
}

#[uniffi::remote(Enum)]
pub enum MediaDelivery {
    Direct,
    Hls,
    Dash,
}

#[uniffi::remote(Enum)]
pub enum VideoCodec {
    Av1,
    Vp9,
    H264,
    Other(String),
}

#[uniffi::remote(Enum)]
pub enum AudioCodec {
    Opus,
    Aac,
    Other(String),
}

#[uniffi::remote(Enum)]
pub enum MediaContainer {
    Mp4,
    Webm,
    M4a,
    Other(String),
}

#[uniffi::remote(Record)]
pub struct MediaFormat {
    pub id: String,
    pub kind: MediaKind,
    pub height: Option<i64>,
    pub width: Option<i64>,
    pub vcodec: Option<VideoCodec>,
    pub acodec: Option<AudioCodec>,
    pub container: MediaContainer,
    pub filesize: Option<i64>,
    pub filesize_approx: Option<i64>,
    pub tbr: Option<f64>,
    pub url: SourceUrl,
    pub delivery: MediaDelivery,
}

#[uniffi::remote(Record)]
pub struct QualityPreferences {
    pub max_height: i64,
    pub video_codecs: HashSet<VideoCodec>,
    pub containers: HashSet<MediaContainer>,
    pub audio_codecs: HashSet<AudioCodec>,
}

#[uniffi::remote(Record)]
pub struct FormatChoice {
    pub video: Option<MediaFormat>,
    pub audio: Option<MediaFormat>,
    pub output_container: MediaContainer,
    pub estimated_bytes: Option<i64>,
    pub wholesale_selector: Option<String>,
}

#[uniffi::remote(Record)]
pub struct ResolvedMedia {
    pub extractor: String,
    pub video_id: String,
    pub title: String,
    pub duration_seconds: Option<f64>,
    pub formats: Vec<MediaFormat>,
    pub source_url: Option<SourceUrl>,
}

#[uniffi::remote(Record)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub is_dark: bool,
    pub source: String,
    pub surface_primary: String,
    pub surface_secondary: String,
    pub surface_tertiary: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_tertiary: String,
    pub accent: String,
    pub border: String,
    pub status_online: String,
    pub status_faulty: String,
    pub status_offline: String,
    pub status_failed: String,
    pub progress_fill: String,
    pub completed_segment_fill: String,
    pub active_head_tint: String,
    pub graph_stroke: String,
    pub graph_average_stroke: String,
    pub sidebar_background: String,
    pub selection_tint: String,
}

// MARK: sdm-resolve

#[uniffi::remote(Enum)]
pub enum CookieSource {
    None,
    Safari,
    Chrome,
    Firefox,
    Edge,
    Brave,
}

#[uniffi::remote(Enum)]
pub enum YtDlpChannel {
    Stable,
    Nightly,
}

#[uniffi::remote(Record)]
pub struct VendorAsset {
    pub name: String,
    pub compressed_path: PathBuf,
    pub version: String,
}

#[uniffi::remote(Enum)]
pub enum CheckReason {
    Launch,
    Timer,
    ResolveNeeded,
    ChannelChanged,
    Manual,
}

#[uniffi::remote(Enum)]
pub enum CheckOutcome {
    UpToDate(Option<String>),
    Updated(String),
    NoNetwork,
    Failed(String),
    Skipped,
}

#[uniffi::remote(Record)]
pub struct ManagedBinariesStatus {
    pub yt_dlp_version: Option<String>,
    pub latest_known: Option<String>,
    pub channel: YtDlpChannel,
    pub ffmpeg_version: Option<String>,
    pub qjs_version: Option<String>,
    pub last_check_tick: Option<i64>,
    pub last_error: Option<String>,
}

// MARK: sdm-engine

#[uniffi::remote(Record)]
pub struct EngineSettings {
    pub max_concurrent: usize,
    pub segments_per_item: usize,
    pub global_max_connections: usize,
    pub max_connections_per_host: usize,
    pub download_folder: PathBuf,
    pub checkpoint_interval_bytes: i64,
    pub persist_debounce_ticks: usize,
    pub min_segment_size_bytes: i64,
    pub file_check_interval_ticks: usize,
}

#[uniffi::remote(Record)]
pub struct PackageAggregate {
    pub completed_bytes: i64,
    pub total_bytes: i64,
    pub bytes_per_second: f64,
    pub speed_history: Vec<f64>,
    pub eta: f64,
}

#[uniffi::remote(Record)]
pub struct EngineFigures {
    pub running_count: usize,
    pub queued_count: usize,
    pub overall_fraction: f64,
    pub ring_completed_bytes: i64,
    pub ring_total_bytes: i64,
    pub waiting_item_count: usize,
    pub waiting_package_count: usize,
}

#[uniffi::remote(Record)]
pub struct ItemSnapshot {
    pub id: Uuid,
    pub url: SourceUrl,
    pub filename: String,
    pub total_bytes: Option<i64>,
    pub completed: RangeSet,
    pub state: ItemState,
    pub is_enabled: bool,
    pub is_resumable: Option<bool>,
    pub active_segments: usize,
    pub configured_segments: usize,
    pub bytes_per_second: f64,
    pub speed_history: Vec<f64>,
    pub checkpoint_failure: Option<String>,
    pub remaining_attempts: Option<usize>,
    pub failed_attempt_count: Option<usize>,
    pub last_failure_reason: Option<String>,
    pub retry_hold_seconds: Option<usize>,
    pub file_missing: bool,
    pub is_assembling: bool,
    pub assembly: Assembly,
    pub part_filenames: Vec<String>,
    pub metadata: Option<String>,
}

#[uniffi::remote(Record)]
pub struct PackageSnapshot {
    pub id: Uuid,
    pub name: String,
    pub priority: Priority,
    pub items: Vec<ItemSnapshot>,
    pub aggregate: PackageAggregate,
    pub revision: u64,
}

#[uniffi::remote(Record)]
pub struct EngineSnapshot {
    pub packages: Vec<PackageSnapshot>,
    pub global_bytes_per_second: f64,
    pub global_history: Vec<f64>,
    pub figures: EngineFigures,
    pub generation: u64,
}

// MARK: sdm-grabber

#[uniffi::remote(Enum)]
pub enum FileSignature {
    Zip,
    Rar,
    Gzip,
    Mp4,
    Mkv,
    Pdf,
    Html,
    Unknown,
}

#[uniffi::remote(Enum)]
pub enum Verdict {
    Online,
    Offline,
    Faulty { reason: String },
    CheckFailed,
}

#[uniffi::remote(Enum)]
pub enum Stage {
    Queued,
    Probing,
    Sniffing,
    Done,
}

#[uniffi::remote(Record)]
pub struct ProbedLink {
    pub id: Uuid,
    pub original_url: SourceUrl,
    pub final_url: SourceUrl,
    pub stage: Stage,
    pub status_code: Option<i64>,
    pub content_length: Option<i64>,
    pub content_type: Option<String>,
    pub suggested_filename: Option<String>,
    pub validator: Option<String>,
    pub accepts_ranges: bool,
    pub sniffed_signature: Option<FileSignature>,
    pub transport_failed: bool,
    pub verdict: Option<Verdict>,
    pub is_duplicate: bool,
}

#[uniffi::remote(Enum)]
pub enum MediaRowState {
    Resolving,
    Resolved,
    Unselected,
    Unsupported,
    NeedsYtDlp,
    NeedsFfmpeg,
    Failed(String),
}

#[uniffi::remote(Record)]
pub struct MediaRow {
    pub id: Uuid,
    pub source_url: SourceUrl,
    pub title: String,
    pub state: MediaRowState,
    pub media: Option<ResolvedMedia>,
    pub choice: Option<FormatChoice>,
    pub playlist_group: Option<Uuid>,
    pub is_duplicate: bool,
}

#[uniffi::remote(Record)]
pub struct PackageCandidate {
    pub id: Uuid,
    pub name: String,
    pub link_ids: Vec<Uuid>,
    pub is_archive: bool,
    pub note: Option<String>,
}

#[uniffi::remote(Record)]
pub struct GrabberSnapshot {
    pub links: Vec<ProbedLink>,
    pub media_rows: Vec<MediaRow>,
    pub packages: Vec<PackageCandidate>,
    pub checked_count: usize,
    pub total_count: usize,
}

#[uniffi::remote(Record)]
pub struct MediaFormatOption {
    pub id: String,
    pub label: String,
    pub choice: FormatChoice,
    pub matches_preferences: bool,
    pub is_wholesale: bool,
}

#[uniffi::remote(Record)]
pub struct GrabberBudget {
    pub global_max_concurrent_probes: usize,
    pub max_concurrent_per_host: usize,
    pub max_concurrent_resolves: usize,
}
