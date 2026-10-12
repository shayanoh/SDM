//! The engine's mutable state, behind the one mutex in [`Inner`](super::Inner).
//!
//! Swift's comment on `snapshotGeneration`, which this module's structure
//! replaces `didSet` with: *Bumped by every write to a stored property
//! `snapshot()` reads. While nothing is running, an unchanged generation means
//! the previous snapshot is still exact, and `snapshot()` returns it without
//! rebuilding anything. Deliberately coarse — one counter for the whole
//! engine rather than per-item dirty flags — so the only way to serve a stale
//! snapshot is to add a snapshot input without the `didSet`, not to miss one
//! of the dozens of places that mutate an item.*
//!
//! Rust has no `didSet`, so every snapshot input is a **private** field of
//! [`EngineState`]. Reads go through `&self` getters; writes go through
//! `&mut self` accessors that bump the generation when handed out. Code
//! outside this module cannot reach a snapshot input without bumping, so a new
//! input cannot be forgotten. The fields that are not snapshot inputs in Swift
//! (`sampled_bytes`, `attempt_start_bytes`, `started_at_tick`, `current_tick`,
//! `ticks_since_pending_change`, `ticks_until_file_check`, `is_shut_down`,
//! `has_restored`, and the snapshot caches) are plain fields.
//!
//! A `_mut` accessor bumps even if the caller then changes nothing — as
//! Swift's `didSet` does for a dictionary write that removes an absent key.
//! Callers that would otherwise bump on every idle tick therefore check
//! first.

use std::collections::{HashMap, HashSet};

use sdm_core::{AppTiming, DownloadItem, DownloadPackage, ItemState, RangeSet};
use uuid::Uuid;

use super::{EngineSettings, Runner};
use crate::engine_snapshot::{EngineSnapshot, PackageSnapshot};
use crate::speed_sampler::SpeedSampler;

pub(crate) struct EngineState {
    // MARK: Snapshot inputs — private on purpose.
    settings: EngineSettings,
    /// Items currently in the `ffmpeg` assembly step. Their `state` stays
    /// `Running`; the snapshot exposes this as `is_assembling`.
    assembling: HashSet<Uuid>,
    packages: Vec<DownloadPackage>,
    runners: HashMap<Uuid, Runner>,
    samplers: HashMap<Uuid, SpeedSampler>,
    global_sampler: SpeedSampler,
    segment_overrides: HashMap<Uuid, usize>,
    /// Consecutive failed attempts per item, reset by any attempt that made
    /// real progress and by success.
    failed_attempts: HashMap<Uuid, usize>,
    /// Ticks of backoff an item still owes before it may be re-desired.
    /// Counted in heartbeat ticks rather than measured against a clock, which
    /// is the same idiom `checkpoint_tick()` uses and keeps the engine
    /// deterministic under test.
    retry_hold_ticks: HashMap<Uuid, usize>,
    /// The most recent transient failure's message per item, surfaced in the
    /// snapshot only while `failed_attempts` still holds the item (i.e. it is
    /// retrying, not yet terminal and not recovered).
    last_failure: HashMap<Uuid, String>,
    /// The last checkpoint failure reported by each item's task, kept after
    /// the runner retires so the reason does not vanish from the UI the
    /// instant the download stops.
    checkpoint_failures: HashMap<Uuid, String>,
    /// Completed items whose destination file was gone at the last check.
    /// Refreshed on a slow tick cadence (`file_check_interval_ticks`) rather
    /// than `stat`-ed for every completed item on every snapshot.
    missing_files: HashSet<Uuid>,
    snapshot_generation: u64,

    // MARK: Not snapshot inputs.
    /// Byte totals already folded into the samplers, so progress read at 1 Hz
    /// is turned into per-window deltas exactly once.
    pub(super) sampled_bytes: HashMap<Uuid, i64>,
    /// Bytes an attempt started from, so a failure that nevertheless moved
    /// bytes can clear the attempt counter.
    pub(super) attempt_start_bytes: HashMap<Uuid, i64>,
    /// The heartbeat tick each currently-tracked runner started on, so
    /// `reconcile()` can tell whether it is still inside spec §6.4's
    /// hysteresis window. Cleared when the runner finishes, in `finish_item`.
    pub(super) started_at_tick: HashMap<Uuid, usize>,
    /// Ticks elapsed since the engine started, driven by `tick()`. Counted
    /// rather than measured against a clock, matching every other
    /// tick-driven mechanism here (`retry_hold_ticks`, checkpoint staleness).
    pub(super) current_tick: usize,
    /// Ticks elapsed since the first unflushed change, or `None` when the
    /// store has nothing pending. Spec §4.2's debounce.
    pub(super) ticks_since_pending_change: Option<usize>,
    /// Ticks left before `missing_files` is refreshed again. Starts at zero
    /// so the first tick checks.
    pub(super) ticks_until_file_check: usize,
    pub(super) is_shut_down: bool,
    pub(super) has_restored: bool,
    /// Items a removal or reset is in the middle of. Between stopping the old
    /// runner and finishing the file work the item sits `Queued` with no
    /// runner, and a `reconcile()` landing in that window would start a new
    /// task on the files being trashed. `schedulable_packages` hides these
    /// from the scheduler. Not a snapshot input and not persisted: nothing
    /// outside the scheduler reads it.
    pub(super) removing: HashSet<Uuid>,
    /// The last snapshot built, and the generation it was built at.
    pub(super) last_snapshot: Option<EngineSnapshot>,
    pub(super) last_snapshot_generation: u64,
    /// Each package's last snapshot, so one whose content came out the same
    /// is handed back as the *same value* — same revision — and consumers
    /// comparing two engine snapshots can skip it by comparing integers.
    pub(super) package_snapshot_cache: HashMap<Uuid, PackageSnapshot>,
    /// The last `PackageSnapshot::revision` handed out.
    pub(super) last_package_revision: u64,
    /// The last `EngineSnapshot::generation` handed out.
    pub(super) last_snapshot_serial: u64,
}

impl EngineState {
    pub(super) fn new(settings: EngineSettings) -> Self {
        Self {
            settings,
            assembling: HashSet::new(),
            packages: Vec::new(),
            runners: HashMap::new(),
            samplers: HashMap::new(),
            global_sampler: SpeedSampler::new()
                .with_history_length(AppTiming::TICKS_PER_SECOND * 300)
                .with_averaging_window_seconds(2.0),
            segment_overrides: HashMap::new(),
            failed_attempts: HashMap::new(),
            retry_hold_ticks: HashMap::new(),
            last_failure: HashMap::new(),
            checkpoint_failures: HashMap::new(),
            missing_files: HashSet::new(),
            snapshot_generation: 0,
            sampled_bytes: HashMap::new(),
            attempt_start_bytes: HashMap::new(),
            started_at_tick: HashMap::new(),
            current_tick: 0,
            ticks_since_pending_change: None,
            ticks_until_file_check: 0,
            is_shut_down: false,
            has_restored: false,
            removing: HashSet::new(),
            last_snapshot: None,
            last_snapshot_generation: 0,
            package_snapshot_cache: HashMap::new(),
            last_package_revision: 0,
            last_snapshot_serial: 0,
        }
    }

    fn bump(&mut self) {
        self.snapshot_generation = self.snapshot_generation.wrapping_add(1);
    }

    pub(super) fn snapshot_generation(&self) -> u64 {
        self.snapshot_generation
    }

    // MARK: Accessors — a getter, and a bumping `_mut` for each snapshot input.

    pub(super) fn settings(&self) -> &EngineSettings {
        &self.settings
    }

    pub(super) fn set_settings(&mut self, settings: EngineSettings) {
        self.bump();
        self.settings = settings;
    }

    pub(super) fn assembling(&self) -> &HashSet<Uuid> {
        &self.assembling
    }

    pub(super) fn assembling_mut(&mut self) -> &mut HashSet<Uuid> {
        self.bump();
        &mut self.assembling
    }

    pub(super) fn packages(&self) -> &[DownloadPackage] {
        &self.packages
    }

    pub(super) fn packages_mut(&mut self) -> &mut Vec<DownloadPackage> {
        self.bump();
        &mut self.packages
    }

    pub(super) fn runners(&self) -> &HashMap<Uuid, Runner> {
        &self.runners
    }

    pub(super) fn runners_mut(&mut self) -> &mut HashMap<Uuid, Runner> {
        self.bump();
        &mut self.runners
    }

    pub(super) fn samplers(&self) -> &HashMap<Uuid, SpeedSampler> {
        &self.samplers
    }

    pub(super) fn samplers_mut(&mut self) -> &mut HashMap<Uuid, SpeedSampler> {
        self.bump();
        &mut self.samplers
    }

    pub(super) fn global_sampler(&self) -> &SpeedSampler {
        &self.global_sampler
    }

    pub(super) fn global_sampler_mut(&mut self) -> &mut SpeedSampler {
        self.bump();
        &mut self.global_sampler
    }

    pub(super) fn segment_overrides_mut(&mut self) -> &mut HashMap<Uuid, usize> {
        self.bump();
        &mut self.segment_overrides
    }

    pub(super) fn failed_attempts(&self) -> &HashMap<Uuid, usize> {
        &self.failed_attempts
    }

    pub(super) fn failed_attempts_mut(&mut self) -> &mut HashMap<Uuid, usize> {
        self.bump();
        &mut self.failed_attempts
    }

    pub(super) fn retry_hold_ticks(&self) -> &HashMap<Uuid, usize> {
        &self.retry_hold_ticks
    }

    pub(super) fn retry_hold_ticks_mut(&mut self) -> &mut HashMap<Uuid, usize> {
        self.bump();
        &mut self.retry_hold_ticks
    }

    pub(super) fn last_failure(&self) -> &HashMap<Uuid, String> {
        &self.last_failure
    }

    pub(super) fn last_failure_mut(&mut self) -> &mut HashMap<Uuid, String> {
        self.bump();
        &mut self.last_failure
    }

    pub(super) fn checkpoint_failures(&self) -> &HashMap<Uuid, String> {
        &self.checkpoint_failures
    }

    pub(super) fn checkpoint_failures_mut(&mut self) -> &mut HashMap<Uuid, String> {
        self.bump();
        &mut self.checkpoint_failures
    }

    pub(super) fn missing_files(&self) -> &HashSet<Uuid> {
        &self.missing_files
    }

    pub(super) fn missing_files_mut(&mut self) -> &mut HashSet<Uuid> {
        self.bump();
        &mut self.missing_files
    }

    // MARK: Helpers that touch the inputs.

    /// The item and its package, if the engine has it.
    pub(super) fn find_item(&self, item_id: Uuid) -> Option<(&DownloadPackage, &DownloadItem)> {
        self.packages.iter().find_map(|package| {
            package
                .items
                .iter()
                .find(|item| item.id == item_id)
                .map(|item| (package, item))
        })
    }

    /// Where the item sits: `(package index, item index)`.
    pub(super) fn location_of(&self, item_id: Uuid) -> Option<(usize, usize)> {
        self.packages.iter().enumerate().find_map(|(p, package)| {
            package
                .items
                .iter()
                .position(|item| item.id == item_id)
                .map(|i| (p, i))
        })
    }

    pub(super) fn item_state(&self, item_id: Uuid) -> Option<ItemState> {
        self.find_item(item_id).map(|(_, item)| item.state.clone())
    }

    pub(super) fn is_item_enabled(&self, item_id: Uuid) -> bool {
        self.find_item(item_id)
            .is_some_and(|(_, item)| item.is_enabled)
    }

    /// Applies `transform` to the item, if present. Bumps the generation only
    /// when the item exists, as a Swift write through `packages[..]` does.
    pub(super) fn mutate_item(&mut self, item_id: Uuid, transform: impl FnOnce(&mut DownloadItem)) {
        let found = self.packages.iter().enumerate().find_map(|(p, package)| {
            package
                .items
                .iter()
                .position(|item| item.id == item_id)
                .map(|i| (p, i))
        });
        if let Some((package_index, item_index)) = found {
            let item = &mut self.packages_mut()[package_index].items[item_index];
            #[cfg(feature = "engine-logging")]
            let before = item.state.clone();
            transform(item);
            #[cfg(feature = "engine-logging")]
            if item.state != before {
                crate::engine_log!(
                    "{} state: {:?} -> {:?}",
                    crate::item_tag(item_id, item.filename()),
                    before,
                    item.state
                );
            }
        }
    }

    pub(super) fn segment_count(&self, item_id: Uuid) -> usize {
        self.segment_overrides
            .get(&item_id)
            .copied()
            .unwrap_or(self.settings.segments_per_item)
    }

    /// What a fresh `DownloadTask` for this item is expected to resume from.
    pub(super) fn completed_ranges_of(&self, item_id: Uuid) -> RangeSet {
        self.find_item(item_id)
            .map(|(_, item)| item.completed())
            .unwrap_or_default()
    }

    /// Renumbers a package's items to a contiguous `0..count` sequence
    /// matching their array order — the same normalization `reorder_items`
    /// already applies to a full reordering, needed here too since `Scheduler`
    /// ranks directly on `item.position`, so an inserted item's position must
    /// not collide with or fall out of order against its new siblings'.
    pub(super) fn renumber_positions(&mut self, package_index: usize) {
        for (index, item) in self.packages_mut()[package_index]
            .items
            .iter_mut()
            .enumerate()
        {
            item.position = index as i64;
        }
    }

    /// Forgets everything the engine tracks about an item that is gone.
    pub(super) fn clear_item_bookkeeping(&mut self, item_id: Uuid) {
        self.failed_attempts_mut().remove(&item_id);
        self.retry_hold_ticks_mut().remove(&item_id);
        self.last_failure_mut().remove(&item_id);
        self.samplers_mut().remove(&item_id);
        self.sampled_bytes.remove(&item_id);
        self.checkpoint_failures_mut().remove(&item_id);
        self.segment_overrides_mut().remove(&item_id);
        self.attempt_start_bytes.remove(&item_id);
        self.started_at_tick.remove(&item_id);
    }

    /// Renumbers every package's position, and every item's position within
    /// it, to match current array order — `packages` array order is what
    /// `snapshot()` actually displays, but `Scheduler::rank` sorts by the
    /// `position` field, not array order, so the two must never drift apart.
    /// `move_item`/`reorder_items`/`reorder_packages` keep that invariant by
    /// construction (they renumber after rearranging), but `add()` and
    /// `restore()` hand `packages`/`items` a caller-supplied struct whose
    /// `position` defaults to `0` regardless of where it lands in the array
    /// — trusting that value let a freshly added package silently collide
    /// with (or outrank) an existing one that already had a real position,
    /// scheduling it out of the order actually shown in the list.
    pub(super) fn renumber_all_positions(&mut self) {
        for (package_index, package) in self.packages_mut().iter_mut().enumerate() {
            package.position = package_index as i64;
            for (item_index, item) in package.items.iter_mut().enumerate() {
                item.position = item_index as i64;
            }
        }
    }

    /// Re-checks which completed items' files are still on disk. One `stat`
    /// per completed item, so it runs on `file_check_interval_ticks` (and once
    /// at restore) instead of inside every `snapshot()`.
    pub(super) fn refresh_missing_files(&mut self) {
        let mut missing = HashSet::new();
        for package in &self.packages {
            let folder = self.settings.download_folder.join(&package.name);
            for item in package
                .items
                .iter()
                .filter(|item| item.state == ItemState::Completed)
            {
                if !folder.join(item.filename()).exists() {
                    missing.insert(item.id);
                }
            }
        }
        // Gated so an unchanged result does not bump the generation.
        if missing != self.missing_files {
            self.missing_files = missing;
            self.bump();
        }
    }

    /// Folds a monotonically growing byte total into the per-item and global
    /// samplers as a delta, so the same bytes are never counted twice.
    ///
    /// A total lower than the baseline is ignored rather than rebased onto.
    /// `tick()` reads a task's total outside the lock, and the job can finish
    /// in the meantime — `finish_item` then records the final figure, and
    /// `tick()` resumes holding a smaller, stale one. Rebasing down there
    /// would let a preempted item's later resume re-record the span between
    /// the two. The baseline is instead seeded deliberately in `reconcile()`,
    /// the only place progress can legitimately go backwards.
    pub(super) fn record_progress(&mut self, transferred: i64, item_id: Uuid) {
        let previous = self.sampled_bytes.get(&item_id).copied().unwrap_or(0);
        if transferred <= previous {
            return;
        }
        self.sampled_bytes.insert(item_id, transferred);
        let delta = transferred - previous;
        if let Some(sampler) = self.samplers_mut().get_mut(&item_id) {
            sampler.record(delta);
        }
        self.global_sampler_mut().record(delta);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sdm_core::SourceUrl;

    use super::*;

    fn state() -> EngineState {
        EngineState::new(EngineSettings::new(1, 1, 8, PathBuf::from("/nonexistent")))
    }

    /// Runs `write` and asserts the generation moved.
    fn assert_bumps(name: &str, state: &mut EngineState, write: impl FnOnce(&mut EngineState)) {
        let before = state.snapshot_generation();
        write(state);
        assert_ne!(
            state.snapshot_generation(),
            before,
            "{name} did not bump the snapshot generation"
        );
    }

    /// Every way to write a snapshot input moves the generation: each `&mut`
    /// accessor the moment it is handed out, and each helper that writes
    /// through one. (The fields themselves are private, so the accessors are
    /// the only way in; a new input needs a line here and a bumping accessor.)
    #[test]
    fn every_snapshot_input_bumps_the_generation() {
        let mut state = state();
        let id = Uuid::new_v4();

        assert_bumps("set_settings", &mut state, |s| {
            s.set_settings(EngineSettings::new(2, 3, 8, PathBuf::from("/nonexistent")))
        });
        assert_bumps("assembling_mut", &mut state, |s| {
            s.assembling_mut();
        });
        assert_bumps("packages_mut", &mut state, |s| {
            s.packages_mut();
        });
        assert_bumps("runners_mut", &mut state, |s| {
            s.runners_mut();
        });
        assert_bumps("samplers_mut", &mut state, |s| {
            s.samplers_mut();
        });
        assert_bumps("global_sampler_mut", &mut state, |s| {
            s.global_sampler_mut();
        });
        assert_bumps("segment_overrides_mut", &mut state, |s| {
            s.segment_overrides_mut();
        });
        assert_bumps("failed_attempts_mut", &mut state, |s| {
            s.failed_attempts_mut();
        });
        assert_bumps("retry_hold_ticks_mut", &mut state, |s| {
            s.retry_hold_ticks_mut();
        });
        assert_bumps("last_failure_mut", &mut state, |s| {
            s.last_failure_mut();
        });
        assert_bumps("checkpoint_failures_mut", &mut state, |s| {
            s.checkpoint_failures_mut();
        });
        assert_bumps("missing_files_mut", &mut state, |s| {
            s.missing_files_mut();
        });

        // The helpers that write inputs.
        let item = DownloadItem::http(
            SourceUrl::new("https://example.com/a.bin").unwrap(),
            "a.bin",
        );
        let item_id = item.id;
        state
            .packages_mut()
            .push(DownloadPackage::new("p").with_items(vec![item]));
        assert_bumps("mutate_item", &mut state, |s| {
            s.mutate_item(item_id, |item| item.is_enabled = false)
        });
        assert_bumps("renumber_positions", &mut state, |s| {
            s.renumber_positions(0)
        });
        assert_bumps("renumber_all_positions", &mut state, |s| {
            s.renumber_all_positions()
        });
        assert_bumps("clear_item_bookkeeping", &mut state, |s| {
            s.clear_item_bookkeeping(id)
        });
        state.samplers_mut().insert(id, SpeedSampler::new());
        assert_bumps("record_progress", &mut state, |s| {
            s.record_progress(100, id)
        });
        // `refresh_missing_files` bumps only when the answer changed: a
        // completed item whose file is not on disk.
        state.mutate_item(item_id, |item| item.state = ItemState::Completed);
        assert_bumps("refresh_missing_files", &mut state, |s| {
            s.refresh_missing_files()
        });
    }

    /// The flip side: reads and an unchanged `refresh_missing_files` leave the
    /// generation alone, so an idle tick does not invalidate the snapshot.
    #[test]
    fn reads_and_an_unchanged_file_check_do_not_bump_the_generation() {
        let mut state = state();
        let before = state.snapshot_generation();
        let _ = (
            state.settings(),
            state.packages(),
            state.runners(),
            state.samplers(),
            state.global_sampler(),
            state.failed_attempts(),
            state.retry_hold_ticks(),
            state.last_failure(),
            state.checkpoint_failures(),
            state.missing_files(),
            state.assembling(),
            state.segment_count(Uuid::new_v4()),
        );
        state.refresh_missing_files();
        assert_eq!(state.snapshot_generation(), before);
    }
}
