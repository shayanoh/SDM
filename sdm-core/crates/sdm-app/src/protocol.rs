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
    pub file_missing: bool,
    pub metadata: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub download_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinariesStatusDto {
    pub yt_dlp_version: Option<String>,
    pub latest_known: Option<String>,
    pub ffmpeg_version: Option<String>,
    pub qjs_version: Option<String>,
    pub last_error: Option<String>,
}
