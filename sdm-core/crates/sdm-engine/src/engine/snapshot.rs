//! Building the engine snapshot the UI consumes.

use std::collections::{HashMap, HashSet};

use sdm_core::{AppTiming, ByteRange, ItemState, Priority, RangeSet};
use uuid::Uuid;

use super::DownloadEngine;
use super::state::EngineState;
use crate::engine_snapshot::{EngineSnapshot, ItemSnapshot, PackageSnapshot};
use crate::snapshot_aggregates::average_recent_speed;
use crate::speed_sampler::SpeedSampler;

/// A package's items, snapshotted, before its aggregate (which needs the
/// whole engine's recent speed) can be computed.
struct PackageItems {
    id: Uuid,
    name: String,
    priority: Priority,
    items: Vec<ItemSnapshot>,
}

impl DownloadEngine {
    pub async fn snapshot(&self) -> EngineSnapshot {
        self.build_snapshot(None)
            .expect("a snapshot with no generation to match is always returned")
    }

    /// The snapshot, unless it is the one the caller already holds:
    /// `None` when its `generation` equals `since_generation`.
    ///
    /// The comparison happens under the engine lock and before the cached
    /// snapshot is cloned, so an idle engine answers a per-tick poll without
    /// copying a list of any length. `generation` only moves when the
    /// content changed (see `snapshot`), so equal generations mean equal
    /// snapshots. `None` for `since_generation` always returns the snapshot.
    pub async fn snapshot_if_changed(
        &self,
        since_generation: Option<u64>,
    ) -> Option<EngineSnapshot> {
        self.build_snapshot(since_generation)
    }

    fn build_snapshot(&self, since_generation: Option<u64>) -> Option<EngineSnapshot> {
        let max_attempts = self.inner.retry_policy.max_attempts;
        let mut guard = self.inner.lock();
        let state = &mut *guard;

        // With no runner there is no live task state to ask, so the result is
        // a function of the engine's own stored properties — and none of
        // those has been written since the last build.
        if state.runners().is_empty()
            && let Some(last) = &state.last_snapshot
            && state.last_snapshot_generation == state.snapshot_generation()
        {
            if since_generation == Some(last.generation) {
                return None;
            }
            return Some(last.clone());
        }
        // Read before the build: the build below is one lock scope, so nothing
        // can bump the counter during it, but the cached build must be tagged
        // with the generation its inputs were read at.
        let built_at_generation = state.snapshot_generation();

        let mut built_packages: Vec<PackageItems> = Vec::new();
        for package in state.packages() {
            let items = package
                .items
                .iter()
                .map(|item| item_snapshot(state, item, max_attempts))
                .collect();
            built_packages.push(PackageItems {
                id: package.id,
                name: package.name.clone(),
                priority: package.priority,
                items,
            });
        }

        // Spec §5.4: the global figure is the sum of the package figures,
        // which are sums of the item figures, so the three can never
        // disagree. The global sampler supplies only the graph history.
        let global_bytes_per_second = built_packages.iter().fold(0.0, |sum, package| {
            sum + package
                .items
                .iter()
                .fold(0.0, |sum, item| sum + item.bytes_per_second)
        });
        let global_history = state.global_sampler().history().to_vec();
        let global_recent_speed = average_recent_speed(&global_history, global_bytes_per_second);

        let mut package_snapshots: Vec<PackageSnapshot> = Vec::new();
        let mut refreshed_cache: HashMap<Uuid, PackageSnapshot> = HashMap::new();
        for package in built_packages {
            let cached_revision = state
                .package_snapshot_cache
                .get(&package.id)
                .map(|cached| cached.revision);
            let candidate = PackageSnapshot::new(
                package.id,
                package.name,
                package.priority,
                package.items,
                global_recent_speed,
                cached_revision.unwrap_or(0),
            );
            let snapshot = settle_revision(
                state.package_snapshot_cache.get(&candidate.id),
                candidate,
                &mut state.last_package_revision,
            );
            refreshed_cache.insert(snapshot.id, snapshot.clone());
            package_snapshots.push(snapshot);
        }
        state.package_snapshot_cache = refreshed_cache;

        let mut result = EngineSnapshot::new(
            package_snapshots,
            global_bytes_per_second,
            global_history,
            state.last_snapshot_serial,
        );
        // Same reuse one level up: an unchanged engine returns the very same
        // snapshot value, `generation` included. Again `content_eq`, so a
        // changed figure or aggregate counts as a change.
        match &state.last_snapshot {
            Some(last) if last.content_eq(&result) => result = last.clone(),
            _ => {
                state.last_snapshot_serial += 1;
                result.generation = state.last_snapshot_serial;
            }
        }
        state.last_snapshot = Some(result.clone());
        state.last_snapshot_generation = built_at_generation;
        if since_generation == Some(result.generation) {
            return None;
        }
        Some(result)
    }

    /// Each requested item's full speed history — the sampler's whole
    /// retained series, not the short tail a snapshot carries. For the one
    /// place that draws a wide graph of a specific item: the details panel.
    /// Items with no sampler are simply absent from the result.
    pub async fn speed_histories(&self, item_ids: &HashSet<Uuid>) -> HashMap<Uuid, Vec<f64>> {
        let state = self.inner.lock();
        item_ids
            .iter()
            .filter_map(|id| {
                state
                    .samplers()
                    .get(id)
                    .map(|sampler| (*id, sampler.history().to_vec()))
            })
            .collect()
    }
}

/// Hands back the previous value when nothing in the freshly built package
/// changed, so its revision is the same as last time; otherwise stamps the
/// next revision on it. Checked by comparing the new value against the cached
/// one — never by tracking what was mutated — so it cannot go stale.
/// `content_eq`, not `==`: `PartialEq` ignores `aggregate`, and an ETA that
/// moved while the items stayed equal still has to reach the UI. (The
/// candidate carries the cached revision, so `content_eq` compares the rest.)
fn settle_revision(
    cached: Option<&PackageSnapshot>,
    candidate: PackageSnapshot,
    last_revision: &mut u64,
) -> PackageSnapshot {
    match cached {
        Some(cached) if cached.content_eq(&candidate) => cached.clone(),
        _ => {
            *last_revision += 1;
            PackageSnapshot {
                revision: *last_revision,
                ..candidate
            }
        }
    }
}

fn item_snapshot(
    state: &EngineState,
    item: &sdm_core::DownloadItem,
    max_attempts: usize,
) -> ItemSnapshot {
    let mut completed = item.completed();
    let mut total_bytes = item.total_bytes();
    let mut active = 0;
    let mut checkpoint_failure = state.checkpoint_failures().get(&item.id).cloned();
    let mut wholesale_post_processing = false;
    if let Some(runner) = state.runners().get(&item.id) {
        // Per-component sizes: a running component's live probed size when it
        // has one, the stored size otherwise. Bases and the item total are
        // both derived from this one array so the concatenated progress and
        // the total can't drift apart.
        let mut sizes: Vec<Option<i64>> = item.components.iter().map(|c| c.total_bytes).collect();
        let mut live_ranges: HashMap<usize, RangeSet> = HashMap::new();
        for component_run in &runner.components {
            let index = component_run.component_index;
            if index >= sizes.len() {
                continue;
            }
            let telemetry = component_run.telemetry();
            if let Some(live) = telemetry.expected_total_bytes {
                sizes[index] = Some(live);
            }
            live_ranges.insert(index, telemetry.completed_ranges);
            active += telemetry.active_worker_count;
            if let Some(failure) = telemetry.last_checkpoint_failure {
                checkpoint_failure = Some(failure);
            }
            if telemetry.is_assembling {
                wholesale_post_processing = true;
            }
        }

        let mut union = RangeSet::new();
        if sizes.iter().all(Option::is_some) {
            let known: Vec<i64> = sizes.iter().map(|size| size.unwrap_or(0)).collect();
            total_bytes = Some(known.iter().sum());
            let mut bases = Vec::with_capacity(known.len());
            let mut running = 0;
            for size in &known {
                bases.push(running);
                running += size;
            }
            for (index, component) in item.components.iter().enumerate() {
                let ranges = live_ranges.get(&index).unwrap_or(&component.completed);
                for range in ranges.ranges() {
                    union.insert(ByteRange::new(
                        range.start + bases[index],
                        range.end + bases[index],
                    ));
                }
            }
        } else {
            // At least one size still unknown — best-effort: sum the live
            // per-component progress without shifting (correct for a
            // one-component item, an approximation otherwise until the probes
            // land).
            for (index, component) in item.components.iter().enumerate() {
                let ranges = live_ranges.get(&index).unwrap_or(&component.completed);
                for range in ranges.ranges() {
                    union.insert(*range);
                }
            }
        }
        completed = union;
    }

    let default_sampler;
    let sampler = match state.samplers().get(&item.id) {
        Some(sampler) => sampler,
        None => {
            default_sampler = SpeedSampler::new();
            &default_sampler
        }
    };
    // Spec-adjacent UI need: a `Completed` item whose file has since been
    // moved or deleted outside SDM should say so rather than silently claiming
    // to be done. Scoped to `Completed` — any other state's file legitimately
    // may not exist yet. Read from the periodically refreshed set, not checked
    // here: see `refresh_missing_files()`.
    let file_missing =
        item.state == ItemState::Completed && state.missing_files().contains(&item.id);
    let failed_attempts = state.failed_attempts().get(&item.id).copied();

    let mut snapshot = ItemSnapshot::new(
        item.id,
        item.source_url.clone(),
        item.filename(),
        total_bytes,
        completed,
        item.state.clone(),
        item.is_enabled,
        item.is_resumable(),
        active,
        state.segment_count(item.id),
        sampler.bytes_per_second(),
        sampler.recent_history().to_vec(),
    );
    snapshot.checkpoint_failure = checkpoint_failure;
    snapshot.remaining_attempts =
        failed_attempts.map(|attempts| max_attempts.saturating_sub(attempts));
    snapshot.failed_attempt_count = failed_attempts;
    // Only meaningful while the item is still retrying — `failed_attempts` is
    // the gate, and it is cleared on success, progress, and terminal failure
    // alike.
    snapshot.last_failure_reason = if failed_attempts.is_some() {
        state.last_failure().get(&item.id).cloned()
    } else {
        None
    };
    snapshot.retry_hold_seconds = state
        .retry_hold_ticks()
        .get(&item.id)
        .map(|ticks| ticks.div_ceil(AppTiming::TICKS_PER_SECOND));
    snapshot.file_missing = file_missing;
    snapshot.is_assembling = state.assembling().contains(&item.id) || wholesale_post_processing;
    snapshot.assembly = item.assembly;
    snapshot.part_filenames = item
        .components
        .iter()
        .map(|component| component.part_filename.clone())
        .collect();
    snapshot.metadata = item.metadata.clone();
    snapshot
}

#[cfg(test)]
mod tests {
    use sdm_core::{RangeSet, SourceUrl};

    use super::*;

    fn package(global_recent_speed: f64, revision: u64) -> PackageSnapshot {
        let running = ItemSnapshot::new(
            Uuid::nil(),
            SourceUrl::new("https://example.com/a.bin").unwrap(),
            "a.bin",
            Some(1000),
            RangeSet::new(),
            ItemState::Running,
            true,
            Some(true),
            1,
            1,
            10.0,
            vec![10.0],
        );
        let queued = ItemSnapshot::new(
            Uuid::from_u128(1),
            SourceUrl::new("https://example.com/b.bin").unwrap(),
            "b.bin",
            Some(1000),
            RangeSet::new(),
            ItemState::Queued,
            true,
            None,
            0,
            1,
            0.0,
            vec![],
        );
        PackageSnapshot::new(
            Uuid::nil(),
            "p",
            Priority::Normal,
            vec![running, queued],
            global_recent_speed,
            revision,
        )
    }

    /// The package's items are equal and only its ETA moved (the queued
    /// remainder is divided by the global rate): it still gets a new
    /// revision, while an identical rebuild keeps the old one.
    #[test]
    fn a_changed_aggregate_gets_a_new_revision_and_an_unchanged_package_keeps_it() {
        let mut last = 7;
        let cached = package(100.0, 7);

        let same = settle_revision(Some(&cached), package(100.0, 7), &mut last);
        assert_eq!(same.revision, 7);
        assert_eq!(last, 7);

        let slower = package(50.0, 7);
        assert_eq!(slower, cached, "only the aggregate differs");
        assert_ne!(slower.aggregate.eta, cached.aggregate.eta);
        let revised = settle_revision(Some(&cached), slower, &mut last);
        assert_eq!(revised.revision, 8);
        assert_eq!(last, 8);
        assert_eq!(revised.aggregate.eta, package(50.0, 0).aggregate.eta);

        let fresh = settle_revision(None, package(100.0, 0), &mut last);
        assert_eq!(fresh.revision, 9);
    }

    /// Pins the idle fast path itself. The published `generation` stays equal
    /// even after a full rebuild with equal content, so the black-box identity
    /// tests cannot see an idle tick that writes a snapshot input; the
    /// internal counter can. A stopped item and a finished item, a settle
    /// period, then 100 ticks (each followed by a read) must not move it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn idle_ticks_do_not_touch_any_snapshot_input() {
        use std::sync::Arc;

        use crate::fake_origin::FakeOrigin;
        use crate::state_store::InMemoryStateStore;
        use crate::{DownloadEngine, EngineSettings, RetryPolicy};
        use sdm_core::{DownloadItem, DownloadPackage};

        let dir = tempfile::tempdir().unwrap();
        let engine = DownloadEngine::new(
            Arc::new(FakeOrigin::new(vec![7u8; 64])),
            Arc::new(InMemoryStateStore::new()),
            EngineSettings::new(1, 1, 8, dir.path().to_path_buf()),
            RetryPolicy::new(),
            None,
            None,
            None,
        );
        let source = SourceUrl::new("https://example.com/f.bin").unwrap();
        let finished = DownloadItem::http(source.clone(), "done.bin");
        let mut stopped = DownloadItem::http(source, "stopped.bin");
        stopped.state = ItemState::Stopped;
        engine
            .add(DownloadPackage::new("P").with_items(vec![finished, stopped]))
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(20), engine.run_until_idle())
            .await
            .expect("the engine settled")
            .unwrap();
        let states: Vec<ItemState> = engine
            .snapshot()
            .await
            .packages
            .iter()
            .flat_map(|package| package.items.iter().map(|item| item.state.clone()))
            .collect();
        assert_eq!(states, [ItemState::Completed, ItemState::Stopped]);

        for _ in 0..10 {
            engine.tick().await;
            engine.snapshot().await;
        }
        let settled = engine.inner.lock().snapshot_generation();
        for round in 0..100 {
            engine.tick().await;
            engine.snapshot().await;
            assert_eq!(
                engine.inner.lock().snapshot_generation(),
                settled,
                "idle tick {round} wrote a snapshot input"
            );
        }
    }
}
