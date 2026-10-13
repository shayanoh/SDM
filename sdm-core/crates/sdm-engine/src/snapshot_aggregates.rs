use sdm_core::{AppTiming, ItemState};

use crate::engine_snapshot::{ItemSnapshot, PackageSnapshot};

/// Sample-window sizes shared by the live models and the views drawing them.
pub enum LiveWindow {}

impl LiveWindow {
    /// How many trailing ticks an ETA's speed is averaged over: ten seconds.
    pub const ETA_TICKS: usize = AppTiming::TICKS_PER_SECOND * 10;
    /// How many trailing samples a package header's summed sparkline keeps.
    /// The header draws it 60 pt wide and `Sparkline` takes one sample per
    /// point, so anything beyond that is never drawn.
    pub const PACKAGE_SPARKLINE_SAMPLES: usize = 64;
    /// How many trailing speed samples an `ItemSnapshot` carries. Enough
    /// for everything a list draws from it — the 48 pt row sparkline, the
    /// package header's sum, and the ETA average — while the sampler's full
    /// ten-minute history stays in the engine until something asks for it
    /// (`DownloadEngine::speed_histories`).
    pub const ITEM_HISTORY_SAMPLES: usize = 64;
}

/// Mean of the trailing `LiveWindow::ETA_TICKS` samples, or `fallback` when
/// there is no history yet.
pub fn average_recent_speed(history: &[f64], fallback: f64) -> f64 {
    if history.is_empty() {
        return fallback;
    }
    let window = &history[history.len().saturating_sub(LiveWindow::ETA_TICKS)..];
    window.iter().fold(0.0, |sum, v| sum + v) / window.len() as f64
}

/// Seconds left at `speed`, or 0 when it cannot be estimated (no speed, or
/// an unknown total). 0 is what every caller already treats as "show
/// nothing".
pub fn estimated_seconds_remaining(total_bytes: i64, completed_bytes: i64, speed: f64) -> f64 {
    if !(speed > 0.0 && total_bytes > 0) {
        return 0.0;
    }
    (total_bytes - completed_bytes) as f64 / speed
}

/// A package's summed figures, computed once per tick by the app's
/// controller rather than in each header's `body`. Same derived-not-stored
/// rule as `PackageSnapshot`'s own aggregates (spec §5.4): always recomputed
/// from the member items, never tracked separately, so it cannot disagree
/// with them. `Default` is Swift's empty `init()`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageAggregate {
    pub completed_bytes: i64,
    pub total_bytes: i64,
    pub bytes_per_second: f64,
    /// The member items' speed histories summed, aligned on their trailing
    /// edge, and capped at `LiveWindow::PACKAGE_SPARKLINE_SAMPLES`.
    pub speed_history: Vec<f64>,
    /// Seconds until the package finishes: the slowest running item, plus
    /// the queued remainder at the global rate. 0 when nothing is running or
    /// it cannot be estimated.
    pub eta: f64,
}

impl PackageAggregate {
    /// `global_recent_speed` is the global download rate averaged over the
    /// ETA window — what queued items are assumed to get.
    pub fn new(items: &[ItemSnapshot], global_recent_speed: f64) -> Self {
        let mut aggregate = Self::default();
        let mut history_length = 0usize;
        let mut running_eta = 0.0f64;
        let mut queued_remaining = 0i64;
        let mut has_running = false;
        for item in items {
            let item_completed = item.completed.total_bytes();
            let item_total = item.total_bytes.unwrap_or(0);
            aggregate.completed_bytes += item_completed;
            aggregate.total_bytes += item_total;
            aggregate.bytes_per_second += item.bytes_per_second;
            history_length = history_length.max(
                item.speed_history
                    .len()
                    .min(LiveWindow::PACKAGE_SPARKLINE_SAMPLES),
            );
            match item.state {
                ItemState::Running => {
                    has_running = true;
                    let speed = average_recent_speed(&item.speed_history, item.bytes_per_second);
                    running_eta = running_eta.max(estimated_seconds_remaining(
                        item_total,
                        item_completed,
                        speed,
                    ));
                }
                ItemState::Queued => {
                    if item_total > 0 {
                        queued_remaining += item_total - item_completed;
                    }
                }
                ItemState::Stopped | ItemState::Completed | ItemState::Failed { .. } => {}
            }
        }

        if history_length > 0 {
            let mut summed = vec![0.0; history_length];
            for item in items.iter().filter(|item| !item.speed_history.is_empty()) {
                let start = item.speed_history.len().saturating_sub(history_length);
                let window = &item.speed_history[start..];
                let padding = history_length - window.len();
                for (offset, value) in window.iter().enumerate() {
                    summed[padding + offset] += value;
                }
            }
            aggregate.speed_history = summed;
        }

        if has_running {
            let queued_eta = if global_recent_speed > 0.0 {
                queued_remaining as f64 / global_recent_speed
            } else {
                0.0
            };
            aggregate.eta = running_eta + queued_eta;
        }
        aggregate
    }
}

/// The whole-engine counts every chrome surface used to re-derive for itself
/// by flat-mapping every item — sidebar badge, overall ring, menu bar icon,
/// Touch Bar. One pass, pure, over a snapshot. `Default` is Swift's empty
/// `init()`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineFigures {
    pub running_count: usize,
    pub queued_count: usize,
    /// Mean completion of every running/queued/completed item with a known
    /// size — the "overall progress" ring.
    pub overall_fraction: f64,
    /// Downloaded / total bytes over that same set of items.
    pub ring_completed_bytes: i64,
    pub ring_total_bytes: i64,
    /// Enabled items that are not completed yet, and how many packages hold
    /// at least one.
    pub waiting_item_count: usize,
    pub waiting_package_count: usize,
}

impl EngineFigures {
    pub fn new(packages: &[PackageSnapshot]) -> Self {
        let mut figures = Self::default();
        let mut fraction_sum = 0.0;
        let mut ring_count = 0usize;
        for package in packages {
            let mut package_has_waiting = false;
            for item in &package.items {
                let is_ring_state = match item.state {
                    ItemState::Running => {
                        figures.running_count += 1;
                        true
                    }
                    ItemState::Queued => {
                        figures.queued_count += 1;
                        true
                    }
                    ItemState::Completed => true,
                    ItemState::Stopped | ItemState::Failed { .. } => false,
                };
                if is_ring_state && let Some(total) = item.total_bytes.filter(|total| *total > 0) {
                    let completed = item.completed.total_bytes();
                    fraction_sum += completed as f64 / total as f64;
                    ring_count += 1;
                    figures.ring_completed_bytes += completed;
                    figures.ring_total_bytes += total;
                }
                if item.is_enabled && item.state != ItemState::Completed {
                    figures.waiting_item_count += 1;
                    package_has_waiting = true;
                }
            }
            if package_has_waiting {
                figures.waiting_package_count += 1;
            }
        }
        figures.overall_fraction = if ring_count > 0 {
            fraction_sum / ring_count as f64
        } else {
            0.0
        };
        figures
    }
}
