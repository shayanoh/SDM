//! Mutations: adding and restoring packages, settings, the per-item
//! enable / start / stop controls, and shutdown.

use sdm_core::{ComponentOrigin, DownloadPackage, ItemState, Priority};
use uuid::Uuid;

use super::state::EngineState;
use super::{ComponentRun, ComponentWorker, DownloadEngine, EngineSettings, RetireIntent};
use crate::speed_sampler::SpeedSampler;

impl DownloadEngine {
    pub async fn add(&self, package: DownloadPackage) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            let new_ids: Vec<Uuid> = package.items.iter().map(|item| item.id).collect();
            let original = state
                .packages()
                .iter()
                .position(|existing| existing.name == package.name);
            match original {
                Some(index) => state.packages_mut()[index].items.extend(package.items),
                None => state.packages_mut().push(package),
            }
            state.renumber_all_positions();
            for id in new_ids {
                if !state.samplers().contains_key(&id) {
                    state.samplers_mut().insert(id, SpeedSampler::new());
                }
            }
        }
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Loads whatever the state store has durably and installs it as the
    /// starting package list. Call once, before the first `tick()`.
    ///
    /// Idempotent via `has_restored` rather than "call only when `packages` is
    /// empty": a second call (e.g. `EngineController`'s heartbeat re-firing
    /// after a window close/reopen that didn't end the process) is a no-op
    /// rather than dropping whatever `add()` may have contributed in the
    /// meantime. The expected call order is `restore()` once, at the head of
    /// the heartbeat, before any UI-driven `add()` can occur — calling
    /// `add()` first is not a case this method defends against, since the
    /// store itself only ever holds the latest full snapshot `persist()` last
    /// wrote, and an `add()`-triggered `persist()` before `restore()` has run
    /// would already have overwritten whatever `restore()` was going to read.
    ///
    /// Two adjustments are made to what comes back from the store, because
    /// both fields describe the *previous* process's runtime state, not
    /// anything true of this one:
    ///
    /// - `Running`/`Queued` becomes `Stopped`. `persist()` already writes
    ///   every non-terminal item as `Stopped` (see its doc comment), so this
    ///   only matters for a file from an older build or one hand-seeded by a
    ///   test — but the invariant "the store never says something is in
    ///   flight or about to run" should hold regardless of how the file got
    ///   here. Nothing in this process is actually running it: the runner
    ///   that made it `Running` died with the last process, and leaving it
    ///   `Running`/`Queued` would make the scheduler believe a slot is
    ///   occupied that is not, or auto-pick it up before the operator's
    ///   "resume on launch" setting has had a say.
    /// - `is_resumable` resets to `None`. It records what the *previous*
    ///   process's probe found; this process has probed nothing yet. The
    ///   byte-level safety net is `DownloadTask::prepare()`'s validator check
    ///   against the `.sdmpart` sidecar, which runs unconditionally on every
    ///   restart regardless of this flag — so resetting it costs nothing
    ///   there. What it does buy: a stale persisted `false` would otherwise
    ///   hand the item spec §6.3's unconditional-claim rule before this
    ///   process has verified anything, making it unpreemptible on faith.
    ///   `None` puts it through the same probe-then-decide path a freshly
    ///   grabbed item takes.
    pub async fn restore(&self) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            if state.has_restored {
                return;
            }
            state.has_restored = true;
        }

        let mut restored = inner.state_store.load().await.packages;
        for package in &mut restored {
            for item in &mut package.items {
                if matches!(item.state, ItemState::Running | ItemState::Queued) {
                    item.state = ItemState::Stopped;
                }
                // A wholesale component stays `false` (slot reserved) until a
                // run confirms yt-dlp's native downloader is in use, exactly
                // as a freshly grabbed one does — see `MediaHandoff`. Every
                // other component resets to `None` (unprobed, preemptible).
                for component in &mut item.components {
                    component.is_resumable =
                        matches!(component.origin, ComponentOrigin::Wholesale { .. })
                            .then_some(false);
                }
            }
        }

        {
            let mut state = inner.lock();
            let restored_ids: Vec<Uuid> = restored
                .iter()
                .flat_map(|package| package.items.iter().map(|item| item.id))
                .collect();
            state.packages_mut().extend(restored);
            // Self-heals any position/array-order drift already sitting in a
            // persisted file from before this normalization existed — see
            // `renumber_all_positions()`.
            state.renumber_all_positions();
            for id in restored_ids {
                if !state.samplers().contains_key(&id) {
                    state.samplers_mut().insert(id, SpeedSampler::new());
                }
            }
            // So the very first snapshot after launch — published before any
            // tick — already knows which finished files have since gone.
            state.refresh_missing_files();
        }
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Applies new settings live. Spec §10.2's "changing the mode applies
    /// immediately" precedent extends to every engine setting: the very next
    /// `reconcile()` this call triggers picks up the new
    /// `max_concurrent`/connection ceilings with no restart needed.
    ///
    /// # Panics
    ///
    /// Panics if `new_settings` breaks one of its preconditions.
    pub async fn update_settings(&self, new_settings: EngineSettings) {
        new_settings.validate();
        self.inner.lock().set_settings(new_settings);
        self.inner.reconcile().await;
    }

    /// User-initiated Disable/Enable. Purely the `is_enabled` axis — see
    /// `ItemState`'s doc comment for why this is independent of `state`.
    ///
    /// Disabling an item that is currently `Queued` or `Running` also stops
    /// it (mirrors `stop_item`'s effect on `state`), since "disabled" must mean
    /// "not running" immediately, not just "not eligible to be picked up
    /// next." Enabling deliberately does *not* resume it — re-enabling only
    /// makes the item startable again; the user (or Resume All) still has to
    /// start it explicitly. That asymmetry is the point: it is what makes
    /// Start disabled-but-visible rather than silently racing a re-enable.
    pub async fn set_enabled(&self, enabled: bool, item_id: Uuid) {
        let inner = &self.inner;
        let retired = {
            let mut state = inner.lock();
            state.mutate_item(item_id, |item| item.is_enabled = enabled);
            let mut retired = Vec::new();
            if !enabled {
                match state.item_state(item_id) {
                    Some(ItemState::Running) => {
                        retired = retire_runner_nonblocking(&mut state, item_id);
                        state.mutate_item(item_id, |item| item.state = ItemState::Stopped);
                    }
                    Some(ItemState::Queued) => {
                        state.mutate_item(item_id, |item| item.state = ItemState::Stopped);
                    }
                    _ => {}
                }
            }
            retired
        };
        pause_without_waiting(retired);
        inner.persist().await;
        inner.reconcile().await;
    }

    /// User-initiated Start: moves a `Stopped`, enabled item into `Queued`
    /// so the scheduler can pick it up. A no-op on anything else — a disabled
    /// item cannot be started (the UI disables the affordance too, this is
    /// the backstop), and starting a `Queued`/`Running` item has nothing to
    /// do.
    pub async fn start_item(&self, item_id: Uuid) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            if state.item_state(item_id) != Some(ItemState::Stopped)
                || !state.is_item_enabled(item_id)
            {
                return;
            }
            state.mutate_item(item_id, |item| item.state = ItemState::Queued);
        }
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Manually retries a terminally `Failed` item. Spec §6.4 promised "a
    /// terminal `failed` state surfaced with a reason and a manual retry
    /// action" — this is that action.
    ///
    /// A no-op for any other state: `Scheduler::is_eligible` excludes
    /// `Failed` unconditionally, so `Queued` is what makes the item reachable
    /// again, not merely re-enabling it. Guarding on state rather than acting
    /// unconditionally means a button wired to every row cannot accidentally
    /// hand a healthy download a fresh retry budget it never asked for.
    pub async fn retry(&self, item_id: Uuid) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            if !matches!(state.item_state(item_id), Some(ItemState::Failed { .. })) {
                return;
            }
            state.mutate_item(item_id, |item| {
                item.state = ItemState::Queued;
                item.is_enabled = true;
            });
            state.failed_attempts_mut().remove(&item_id);
            state.retry_hold_ticks_mut().remove(&item_id);
        }
        inner.persist().await;
        inner.reconcile().await;
    }

    /// User-initiated Stop: moves a `Queued` or `Running` item to `Stopped`,
    /// stopping its runner first if it has one. Does not touch `is_enabled` —
    /// a stopped-but-enabled item is exactly what Resume All looks for. A
    /// no-op on `Completed`/`Failed`/already-`Stopped` items.
    pub async fn stop_item(&self, item_id: Uuid) {
        let inner = &self.inner;
        let retired = {
            let mut state = inner.lock();
            match state.item_state(item_id) {
                Some(ItemState::Running) => {
                    let retired = retire_runner_nonblocking(&mut state, item_id);
                    state.mutate_item(item_id, |item| item.state = ItemState::Stopped);
                    retired
                }
                Some(ItemState::Queued) => {
                    state.mutate_item(item_id, |item| item.state = ItemState::Stopped);
                    Vec::new()
                }
                _ => return,
            }
        };
        pause_without_waiting(retired);
        inner.persist().await;
        inner.reconcile().await;
    }

    pub async fn set_priority(&self, priority: Option<Priority>, item_id: Uuid) {
        let inner = &self.inner;
        inner
            .lock()
            .mutate_item(item_id, |item| item.priority = priority);
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Global pause: exactly "select every item and click Stop," nothing
    /// more — same `stop_item` call, same eligibility, per item. Never
    /// touches `is_enabled`.
    pub async fn pause_all(&self) {
        for item_id in self.all_item_ids() {
            self.stop_item(item_id).await;
        }
    }

    /// Global resume: exactly "select every item and click Start," nothing
    /// more — same `start_item` call, same eligibility (enabled and
    /// `Stopped`), per item. A disabled item is left alone, same as it
    /// would be by an individual Start.
    pub async fn resume_all(&self) {
        for item_id in self.all_item_ids() {
            self.start_item(item_id).await;
        }
    }

    fn all_item_ids(&self) -> Vec<Uuid> {
        self.inner
            .lock()
            .packages()
            .iter()
            .flat_map(|package| package.items.iter().map(|item| item.id))
            .collect()
    }

    /// # Panics
    ///
    /// Panics if `count` is zero ("segment count must be at least 1").
    pub async fn set_segment_count(&self, count: usize, item_id: Uuid) {
        assert!(count >= 1, "segment count must be at least 1");
        let components = {
            let mut state = self.inner.lock();
            state.segment_overrides_mut().insert(item_id, count);
            state
                .runners()
                .get(&item_id)
                .filter(|runner| runner.retire_intent == RetireIntent::None)
                .map(|runner| runner.components.clone())
        };
        if let Some(components) = components {
            let per_component = (count / components.len().max(1)).max(1);
            for component in &components {
                component.set_worker_count(per_component);
            }
        }
    }

    /// Stops everything and writes durable state. Jobs are awaited rather than
    /// merely cancelled, so no file descriptor outlives the call and each
    /// item's final state reaches the snapshot before it is flushed.
    ///
    /// Idempotent. The app now shuts down from two places — the heartbeat's
    /// `.task` continuation and `applicationWillTerminate` — and on a ⌘Q both
    /// can fire. Guarding here rather than only at the call site makes "call
    /// it once" a property of the engine instead of an invariant every caller
    /// has to uphold.
    pub async fn shutdown(&self) {
        let inner = &self.inner;
        let live = {
            let mut state = inner.lock();
            if state.is_shut_down {
                return;
            }
            state.is_shut_down = true;
            state.runners().values().cloned().collect::<Vec<_>>()
        };
        for runner in &live {
            runner.cancel.cancel();
        }
        for runner in &live {
            runner.pause_all_components().await;
        }
        for runner in &live {
            runner.done.wait().await;
        }
        inner.persist().await;
        inner.lock().ticks_since_pending_change = None;
        inner.state_store.flush().await;
    }

    /// Test helper: pumps the engine until nothing is left running.
    ///
    /// Does not tick, so retry backoff never ages: a transiently failing item
    /// is held out of the desired set and this returns rather than looping.
    /// Still bounded, as a backstop against any future path that could keep
    /// re-desiring an item without a tick.
    #[doc(hidden)]
    pub async fn run_until_idle(&self) -> Result<(), crate::DownloadError> {
        let inner = &self.inner;
        let mut in_flight = inner.reconcile().await;
        for _ in 0..1000 {
            let Some(done) = in_flight else {
                return Ok(());
            };
            done.wait().await;
            in_flight = inner.reconcile().await;
        }
        Err(crate::DownloadError::IncompleteAfterWorkersFinished)
    }
}

/// Marks a running item's runner as user-stopped and cancels its job
/// **without** waiting for it to actually finish unwinding — the user-facing
/// Stop/Disable path. Returns the components to pause, which the caller does
/// once the lock is released (see [`pause_without_waiting`]).
///
/// Blocking here would hang the caller for as long as the worker's in-flight
/// read takes to resolve — against an origin that is merely slow, or a
/// `Probe`-phase test gate that has not opened yet, that is indefinite. That
/// defeats the point of a user-facing Stop, which must return immediately
/// even against a hung read. The runner stays in `runners` (now marked
/// `UserStopped`) until the job completes on its own schedule; its landing
/// state is then decided in `run_item`. Until that happens, the item cannot be
/// restarted (`reconcile()` only starts an item once `runners` no longer has
/// it), the same transient unavailability a preempted-but-not-yet-unwound item
/// already has.
///
/// Empty when the item has no runner or it is already retiring.
pub(super) fn retire_runner_nonblocking(
    state: &mut EngineState,
    item_id: Uuid,
) -> Vec<ComponentRun> {
    let already_retiring = state
        .runners()
        .get(&item_id)
        .is_none_or(|runner| runner.retire_intent != RetireIntent::None);
    if already_retiring {
        return Vec::new();
    }
    let Some(runner) = state.runners_mut().get_mut(&item_id) else {
        return Vec::new();
    };
    runner.retire_intent = RetireIntent::UserStopped;
    runner.cancel.cancel();
    runner.components.clone()
}

/// Pauses components without making the caller wait for a slow stop. A
/// segmented task pauses synchronously, right away; a wholesale one has to
/// wait for its downloader call to stop, so that wait runs on its own task
/// (Swift's unstructured `Task { ... }`). Pausing the segmented ones inline,
/// rather than on a spawned task, means a stop that lands during a download's
/// probe is already recorded before the probe returns.
pub(super) fn pause_without_waiting(components: Vec<ComponentRun>) {
    let mut wholesale = Vec::new();
    for component in components {
        match &component.worker {
            ComponentWorker::Segmented(task) => task.pause(),
            ComponentWorker::Wholesale(_) => wholesale.push(component),
        }
    }
    if !wholesale.is_empty() {
        tokio::spawn(async move {
            for component in &wholesale {
                component.pause().await;
            }
        });
    }
}
