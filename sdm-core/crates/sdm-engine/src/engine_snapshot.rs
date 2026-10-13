use sdm_core::{Assembly, ItemState, Priority, RangeSet, SourceUrl};
use uuid::Uuid;

use crate::snapshot_aggregates::{EngineFigures, PackageAggregate};

/// An immutable view of engine state, published to the UI. Views consume these
/// rather than touching engine state. See spec §5.4 and §9.6.
///
/// Equality is value equality of every field, as in Swift.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSnapshot {
    pub id: Uuid,
    pub url: SourceUrl,
    pub filename: String,
    pub total_bytes: Option<i64>,
    pub completed: RangeSet,
    pub state: ItemState,
    pub is_enabled: bool,
    /// Three-state, mirroring `DownloadItem::is_resumable`: `None` means the
    /// origin has not been probed yet. Not flattened, because the UI badge
    /// has to distinguish "cannot be resumed" from "don't know yet" — they
    /// warrant different affordances, and only the first is a warning.
    pub is_resumable: Option<bool>,
    pub active_segments: usize,
    pub configured_segments: usize,
    pub bytes_per_second: f64,
    /// The trailing `LiveWindow::ITEM_HISTORY_SAMPLES` speed samples — enough
    /// for a row's sparkline and ETA, not the sampler's whole history. The
    /// full series comes from `DownloadEngine::speed_histories`.
    pub speed_history: Vec<f64>,
    /// Why this download's resume state could not be written, or `None` when
    /// checkpointing is healthy. Surfaced because a sidecar that silently
    /// fails to write means a crash loses all progress with no signal — the
    /// user needs to know the download is not actually resumable right now.
    pub checkpoint_failure: Option<String>,
    /// Attempts left before this item becomes terminally `Failed`, or `None`
    /// if it has never failed. Spec §6.4's "manual retry action" needs this
    /// to show the operator how much budget is left before giving up.
    pub remaining_attempts: Option<usize>,
    /// Consecutive failed attempts so far, or `None` once an attempt succeeds
    /// or makes real progress (the engine resets its counter then). `Some` on
    /// a `Queued` item means it is mid-retry, not freshly enqueued.
    pub failed_attempt_count: Option<usize>,
    /// The most recent transient failure's message while the item is retrying,
    /// or `None` when it is not. A terminal `Failed` already carries its
    /// reason in `state`; this is for the retrying-but-not-yet-terminal gap.
    pub last_failure_reason: Option<String>,
    /// Seconds left on the backoff hold before the next attempt, or `None` when
    /// the item is not being held. Lets the UI say "retrying in Ns".
    pub retry_hold_seconds: Option<usize>,
    /// True only for a `Completed` item whose destination file is no longer
    /// on disk — moved or deleted outside SDM. Always `false` for any other
    /// state.
    pub file_missing: bool,
    /// True while the engine is running `ffmpeg` to assemble a muxed item's
    /// downloaded parts. The item's `state` is still `Running`. Parent
    /// spec §7.2.
    pub is_assembling: bool,
    /// What happens once every component completes — `None` for a plain
    /// download, `Mux` for a video+audio YouTube item. Lets the UI show
    /// "Assembling…" and offer "Retry Mux" on a failed mux.
    pub assembly: Assembly,
    /// Each component's part-file name (`clip.f137.mp4`, `clip.f251.webm`).
    /// For a one-component item this is `[filename]`. The delete dialog and
    /// "reveal in Finder" need the real on-disk names, not just the output
    /// filename.
    pub part_filenames: Vec<String>,
    /// A one-line media description shown in the details panel — delivery /
    /// resolution / codecs for a resolver-backed item, release tags mined
    /// from the filename for a plain HTTP one. `None` / empty ⇒ show a dash.
    pub metadata: Option<String>,
}

impl ItemSnapshot {
    /// The twelve required Swift initializer arguments. The defaulted ones
    /// (`checkpointFailure` … `metadata`) start at their Swift defaults —
    /// `None`, `false`, `Assembly::None`, and `part_filenames` of
    /// `[filename]` — and are set with the `with_*` builders.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        url: SourceUrl,
        filename: impl Into<String>,
        total_bytes: Option<i64>,
        completed: RangeSet,
        state: ItemState,
        is_enabled: bool,
        is_resumable: Option<bool>,
        active_segments: usize,
        configured_segments: usize,
        bytes_per_second: f64,
        speed_history: Vec<f64>,
    ) -> Self {
        let filename = filename.into();
        Self {
            id,
            url,
            part_filenames: vec![filename.clone()],
            filename,
            total_bytes,
            completed,
            state,
            is_enabled,
            is_resumable,
            active_segments,
            configured_segments,
            bytes_per_second,
            speed_history,
            checkpoint_failure: None,
            remaining_attempts: None,
            failed_attempt_count: None,
            last_failure_reason: None,
            retry_hold_seconds: None,
            file_missing: false,
            is_assembling: false,
            assembly: Assembly::None,
            metadata: None,
        }
    }

    pub fn with_checkpoint_failure(mut self, failure: impl Into<String>) -> Self {
        self.checkpoint_failure = Some(failure.into());
        self
    }

    pub fn with_remaining_attempts(mut self, attempts: usize) -> Self {
        self.remaining_attempts = Some(attempts);
        self
    }

    pub fn with_failed_attempt_count(mut self, count: usize) -> Self {
        self.failed_attempt_count = Some(count);
        self
    }

    pub fn with_last_failure_reason(mut self, reason: impl Into<String>) -> Self {
        self.last_failure_reason = Some(reason.into());
        self
    }

    pub fn with_retry_hold_seconds(mut self, seconds: usize) -> Self {
        self.retry_hold_seconds = Some(seconds);
        self
    }

    pub fn with_file_missing(mut self, file_missing: bool) -> Self {
        self.file_missing = file_missing;
        self
    }

    pub fn with_is_assembling(mut self, is_assembling: bool) -> Self {
        self.is_assembling = is_assembling;
        self
    }

    pub fn with_assembly(mut self, assembly: Assembly) -> Self {
        self.assembly = assembly;
        self
    }

    /// An empty list keeps the default `[filename]`, as in Swift.
    pub fn with_part_filenames(mut self, part_filenames: Vec<String>) -> Self {
        if !part_filenames.is_empty() {
            self.part_filenames = part_filenames;
        }
        self
    }

    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    pub fn fraction_completed(&self) -> f64 {
        let Some(total) = self.total_bytes.filter(|total| *total > 0) else {
            return 0.0;
        };
        self.completed.total_bytes() as f64 / total as f64
    }

    /// A `Queued` item that has already failed at least once — waiting on a
    /// backoff hold before its next attempt, not freshly enqueued.
    pub fn is_retrying(&self) -> bool {
        self.state == ItemState::Queued && self.failed_attempt_count.is_some()
    }
}

/// One package's view.
///
/// **Equality** (`PartialEq`) is the Swift `Equatable`: `id`, `name`,
/// `priority` and `items`. It deliberately ignores the two fields Rust adds,
/// `aggregate` and `revision`, so two otherwise-identical snapshots stay equal
/// whatever revision the engine stamped on them (and `aggregate` is a pure
/// function of `items` plus a global rate the Swift value never held). Use
/// [`content_eq`](Self::content_eq) to compare every field.
#[derive(Debug, Clone)]
pub struct PackageSnapshot {
    pub id: Uuid,
    pub name: String,
    pub priority: Priority,
    pub items: Vec<ItemSnapshot>,
    /// The package's summed figures, computed by `PackageAggregate::new` at
    /// construction (same code Swift runs per tick in the app controller).
    pub aggregate: PackageAggregate,
    /// Set by the engine: changes only when this package's content changed.
    pub revision: u64,
}

impl PackageSnapshot {
    /// Builds the snapshot and its `aggregate` from `items`.
    /// `global_recent_speed` is the global rate averaged over the ETA window
    /// (see `PackageAggregate::new`); `revision` is the engine's counter.
    pub fn new(
        id: Uuid,
        name: impl Into<String>,
        priority: Priority,
        items: Vec<ItemSnapshot>,
        global_recent_speed: f64,
        revision: u64,
    ) -> Self {
        let aggregate = PackageAggregate::new(&items, global_recent_speed);
        Self {
            id,
            name: name.into(),
            priority,
            items,
            aggregate,
            revision,
        }
    }

    /// Spec §5.4: aggregates are computed from the item figures, never stored
    /// alongside them, so package and global speed cannot disagree with the
    /// per-item numbers they are made of.
    pub fn bytes_per_second(&self) -> f64 {
        self.items
            .iter()
            .fold(0.0, |sum, item| sum + item.bytes_per_second)
    }

    pub fn completed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == ItemState::Completed)
            .count()
    }

    pub fn total_bytes(&self) -> i64 {
        self.items
            .iter()
            .fold(0, |sum, item| sum + item.total_bytes.unwrap_or(0))
    }

    /// Sum of each item's downloaded bytes so far, for the package header's
    /// "x MB / y MB" aggregate. Same derived-not-stored idiom as
    /// `bytes_per_second`.
    pub fn completed_bytes(&self) -> i64 {
        self.items
            .iter()
            .fold(0, |sum, item| sum + item.completed.total_bytes())
    }

    /// Spec §9.6's per-row sparkline data, aggregated the same way
    /// `bytes_per_second` is: summed from the member items, never stored
    /// separately, so it cannot disagree with them. Shorter histories (an
    /// item added mid-run) are aligned to the trailing edge and front-padded
    /// with zero rather than misaligned by index.
    pub fn bytes_per_second_history(&self) -> Vec<f64> {
        let length = self
            .items
            .iter()
            .map(|item| item.speed_history.len())
            .max()
            .unwrap_or(0);
        if length == 0 {
            return Vec::new();
        }
        let mut summed = vec![0.0; length];
        for item in &self.items {
            let padding = length - item.speed_history.len();
            for (index, value) in item.speed_history.iter().enumerate() {
                summed[index + padding] += value;
            }
        }
        summed
    }

    /// Equality over every field, including `aggregate` and `revision`.
    pub fn content_eq(&self, other: &Self) -> bool {
        self == other && self.aggregate == other.aggregate && self.revision == other.revision
    }
}

impl PartialEq for PackageSnapshot {
    /// Swift's `Equatable`; see the type docs for what is left out.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.priority == other.priority
            && self.items == other.items
    }
}

/// The whole engine's view.
///
/// **Equality** (`PartialEq`) is the Swift `Equatable`: `packages`,
/// `global_bytes_per_second` and `global_history`. It ignores the two fields
/// Rust adds, `figures` (a pure function of `packages`) and `generation`
/// (the engine's change counter). [`content_eq`](Self::content_eq) compares
/// every field, with packages compared by `PackageSnapshot::content_eq`.
#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub packages: Vec<PackageSnapshot>,
    pub global_bytes_per_second: f64,
    pub global_history: Vec<f64>,
    /// Whole-engine counts, computed by `EngineFigures::new` at construction.
    pub figures: EngineFigures,
    /// Set by the engine: bumps whenever any snapshot input changed.
    pub generation: u64,
}

impl EngineSnapshot {
    /// Builds the snapshot and its `figures` from `packages`.
    pub fn new(
        packages: Vec<PackageSnapshot>,
        global_bytes_per_second: f64,
        global_history: Vec<f64>,
        generation: u64,
    ) -> Self {
        let figures = EngineFigures::new(&packages);
        Self {
            packages,
            global_bytes_per_second,
            global_history,
            figures,
            generation,
        }
    }

    /// Equality over every field, including `figures`, `generation`, and each
    /// package's `aggregate` and `revision`.
    pub fn content_eq(&self, other: &Self) -> bool {
        self == other
            && self.figures == other.figures
            && self.generation == other.generation
            && self
                .packages
                .iter()
                .zip(&other.packages)
                .all(|(a, b)| a.content_eq(b))
    }
}

impl PartialEq for EngineSnapshot {
    /// Swift's `Equatable`; see the type docs for what is left out.
    fn eq(&self, other: &Self) -> bool {
        self.packages == other.packages
            && self.global_bytes_per_second == other.global_bytes_per_second
            && self.global_history == other.global_history
    }
}
