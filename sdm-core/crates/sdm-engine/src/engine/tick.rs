//! The one-second heartbeat.

use std::collections::HashSet;

use sdm_core::ItemState;
use uuid::Uuid;

use super::{ComponentRun, DownloadEngine, Inner, RetireIntent};

impl DownloadEngine {
    /// One-second heartbeat: folds the window's bytes into the samplers,
    /// drives the wall-clock half of the sidecar checkpoint trigger, closes
    /// the speed window, ages retry backoff, writes debounced durable state,
    /// and reschedules.
    pub async fn tick(&self) {
        let inner = &self.inner;
        let live: Vec<(Uuid, Vec<ComponentRun>)> = {
            let mut state = inner.lock();
            state.current_tick += 1;
            state
                .runners()
                .iter()
                .filter(|(_, runner)| runner.retire_intent == RetireIntent::None)
                .map(|(id, runner)| (*id, runner.components.clone()))
                .collect()
        };
        for (item_id, components) in live {
            let mut transferred: i64 = 0;
            let mut failures: Vec<String> = Vec::new();
            for component in &components {
                transferred += component.completed_ranges().total_bytes();
                // Spec §4.3: "every ~8 MB per worker or every 5 s, whichever
                // comes first". `DownloadTask`'s `record` implements the byte
                // half; this is the only caller of the wall-clock half.
                component.checkpoint_tick();
                if let Some(failure) = component.last_checkpoint_failure() {
                    failures.push(failure);
                }
            }
            let mut state = inner.lock();
            for failure in failures {
                state.checkpoint_failures_mut().insert(item_id, failure);
            }
            // The item may have been removed while its bytes were being read
            // above, outside the lock; recording then would resurrect its
            // bookkeeping and credit the global sampler with a ghost.
            if state.find_item(item_id).is_some() {
                state.record_progress(transferred, item_id);
            }
        }

        {
            let mut state = inner.lock();
            // Spec's "immediately drop to zero when not downloading": a
            // sampler whose item is not `Running` gets `idle()`, not `tick()`,
            // so it reports zero on the very next read instead of decaying
            // toward it. A sampler that still has bytes recorded but not yet
            // folded into a window gets one real `tick()` regardless of the
            // item's current state, so a pause/completion-moment burst — or a
            // download that goes `Queued` -> `Running` -> `Completed` entirely
            // between two heartbeats — still lands in history exactly once
            // instead of being silently dropped.
            //
            // Decided in one read-only pass and applied afterwards, touching
            // only the samplers that actually change: an idle sampler is left
            // alone entirely, so a tick with nothing running writes nothing a
            // snapshot reads (see `state`'s module docs).
            let running_ids: HashSet<Uuid> = state
                .packages()
                .iter()
                .flat_map(|package| &package.items)
                .filter(|item| item.state == ItemState::Running)
                .map(|item| item.id)
                .collect();
            let mut to_tick: Vec<Uuid> = Vec::new();
            let mut to_idle: Vec<Uuid> = Vec::new();
            for (item_id, sampler) in state.samplers() {
                if running_ids.contains(item_id) || sampler.has_pending_bytes() {
                    to_tick.push(*item_id);
                } else if sampler.is_active() || sampler.is_decaying() {
                    to_idle.push(*item_id);
                }
            }
            // Sliding zeros into an idle item's full history only matters to
            // the detail graph; the snapshot generation moves only while the
            // short window a snapshot reads is still changing.
            let snapshot_sees_idle = to_idle.iter().any(|item_id| {
                state
                    .samplers()
                    .get(item_id)
                    .is_some_and(|sampler| sampler.is_active() || sampler.recent_is_decaying())
            });
            if !to_tick.is_empty() || snapshot_sees_idle {
                let samplers = state.samplers_mut();
                for item_id in &to_tick {
                    if let Some(sampler) = samplers.get_mut(item_id) {
                        sampler.tick();
                    }
                }
                for item_id in &to_idle {
                    if let Some(sampler) = samplers.get_mut(item_id) {
                        sampler.idle();
                    }
                }
            } else if !to_idle.is_empty() {
                let samplers = state.samplers_mut_unbumped();
                for item_id in &to_idle {
                    if let Some(sampler) = samplers.get_mut(item_id) {
                        sampler.idle();
                    }
                }
            }
            let any_item_active = !to_tick.is_empty();
            // Mirrors the per-item branch above: without it, the global
            // sampler mutated its history every tick forever (even appending
            // zeros), which kept `EngineSnapshot::global_history` changing and
            // forced the bandwidth graph to fully re-render at the tick rate
            // with no downloads running at all.
            if any_item_active || state.global_sampler().has_pending_bytes() {
                state.global_sampler_mut().tick();
            } else if state.global_sampler().is_active() {
                state.global_sampler_mut().idle();
            }

            if state.ticks_until_file_check == 0 {
                state.refresh_missing_files();
                state.ticks_until_file_check = state.settings().file_check_interval_ticks;
            }
            state.ticks_until_file_check -= 1;

            if !state.retry_hold_ticks().is_empty() {
                let holds = state.retry_hold_ticks_mut();
                holds.retain(|_, remaining| {
                    if *remaining <= 1 {
                        false
                    } else {
                        *remaining -= 1;
                        true
                    }
                });
            }
        }

        inner.flush_if_debounce_elapsed().await;
        inner.reconcile().await;
    }
}

impl Inner {
    /// Spec §4.2: durable state is written "~2 s after the last change".
    ///
    /// `save()` only stores into the store's in-memory `pending`; `flush()` is
    /// the only thing that writes `state.json`. Before this existed the sole
    /// caller of `flush()` anywhere was `shutdown()`, which in the ordinary
    /// quit path never ran — so nothing was ever written, `.incomplete` files
    /// survived with no record of the item owning them, and the next launch
    /// restored an empty list.
    ///
    /// Counted from the *first* unflushed change rather than the last, so a
    /// download that changes something every tick still gets written every two
    /// seconds instead of being starved forever. Cheap when nothing is dirty:
    /// `ticks_since_pending_change` is `None` and this returns without
    /// touching the store.
    async fn flush_if_debounce_elapsed(&self) {
        let should_flush = {
            let mut state = self.lock();
            let Some(elapsed) = state.ticks_since_pending_change else {
                return;
            };
            let next = elapsed + 1;
            if next >= state.settings().persist_debounce_ticks {
                state.ticks_since_pending_change = None;
                true
            } else {
                state.ticks_since_pending_change = Some(next);
                false
            }
        };
        if should_flush {
            self.state_store.flush().await;
        }
    }
}
