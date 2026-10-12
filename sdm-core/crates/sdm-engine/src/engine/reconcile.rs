//! Scheduling: applies the scheduler's desired running set.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use sdm_core::{
    ComponentOrigin, ConnectionAllocator, ConnectionBudget, ConnectionDemand, DownloadPackage,
    ItemState, RangeSet, SourceUrl,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::state::EngineState;
use super::{
    ComponentRun, ComponentWorker, EngineError, Inner, JobDone, JobDoneGuard, RetireIntent, Runner,
};
use crate::download_task::{DownloadTask, DownloadTaskConfiguration};
use crate::scheduler::{Scheduler, SchedulerInput};
use crate::wholesale_component_task::WholesaleComponentTask;

impl Inner {
    /// Applies the scheduler's desired running set: starts what should run,
    /// retires what should not.
    ///
    /// The desired set is computed and applied to `runners` without any
    /// suspension in between — here, inside one lock scope with no `.await`
    /// — so two overlapping calls can never both decide to start the same
    /// item. `started_recently` is derived from `started_at_tick` and the
    /// tick-counted hysteresis window (spec §6.4), rather than a wall clock —
    /// see `current_tick`.
    ///
    /// `refresh_resumability()` runs *before* the decision. Swift's version
    /// suspends there; this one cannot (the task getters are synchronous), so
    /// it simply shares the scope. What must not happen — and does not — is a
    /// suspension between deciding the desired set and acting on it.
    ///
    /// Returns the completion signal of one runner that was in flight when
    /// the decision was applied, or `None` if there was none. Only
    /// `run_until_idle` reads it: taking the runner in the same lock scope as
    /// the decision is what lets it tell "idle" from "a retiring runner
    /// unwound a moment after this call declined to restart its item". Swift
    /// reads `runners.values.first` after `reconcile()` returns, which on one
    /// actor is close enough; across threads that gap is real, and a test
    /// would see the engine report idle with a queued item nothing had
    /// started.
    pub(super) async fn reconcile(self: &Arc<Self>) -> Option<JobDone> {
        let outcome = {
            let mut state = self.lock();
            if state.is_shut_down {
                return None;
            }
            self.decide_and_apply(&mut state)
        };

        // Re-clamp every already-running item too, not just fresh starts: a
        // sibling finishing frees budget the survivors should grow back into,
        // and a newly-added item can just as easily squeeze existing ones
        // down.
        for (components, per_component) in &outcome.resizes {
            for component in components {
                component.set_worker_count(*per_component);
            }
        }
        for component in &outcome.retiring {
            component.pause().await;
        }
        if outcome.changed {
            self.persist().await;
        }
        outcome.in_flight
    }

    /// The body of `reconcile()` that must not suspend.
    fn decide_and_apply(self: &Arc<Self>, state: &mut EngineState) -> Applied {
        refresh_resumability(state);

        let running_now: HashSet<Uuid> = state.runners().keys().copied().collect();
        let started_recently: HashSet<Uuid> = state
            .started_at_tick
            .iter()
            .filter(|(_, started)| {
                state.current_tick.saturating_sub(**started) < self.hysteresis_window_ticks
            })
            .map(|(id, _)| *id)
            .collect();
        let desired = Scheduler::desired_running_set(SchedulerInput::new(
            schedulable_packages(state),
            running_now.clone(),
            started_recently,
            state.settings().max_concurrent,
        ));

        #[cfg(feature = "engine-logging")]
        if desired != running_now {
            crate::engine_log!(
                "[scheduler] assigning: +{} -{} (desired={} running={})",
                desired.difference(&running_now).count(),
                running_now.difference(&desired).count(),
                desired.len(),
                running_now.len()
            );
        }

        let allocated_segments = ConnectionAllocator::allocate(
            &connection_demands(state, &desired),
            &ConnectionBudget::new(
                state.settings().global_max_connections,
                state.settings().max_connections_per_host,
            ),
        );

        let mut changed = false;
        let mut retiring: Vec<ComponentRun> = Vec::new();
        let to_retire: Vec<Uuid> = state
            .runners()
            .iter()
            .filter(|(id, runner)| {
                !desired.contains(id) && runner.retire_intent == RetireIntent::None
            })
            .map(|(id, _)| *id)
            .collect();
        for item_id in to_retire {
            if let Some(runner) = state.runners_mut().get_mut(&item_id) {
                runner.retire_intent = RetireIntent::Preempted;
                runner.cancel.cancel();
                retiring.extend(runner.components.iter().cloned());
            }
            state.mutate_item(item_id, |item| item.state = ItemState::Queued);
            changed = true;
        }

        // Two tasks writing the same destination would interleave at absolute
        // offsets and corrupt it. Retiring runners are still in `runners`, so
        // this also covers restarting an item whose previous task has not
        // unwound yet.
        let mut claimed: HashSet<PathBuf> = state
            .runners()
            .values()
            .flat_map(|runner| runner.all_destinations().cloned())
            .collect();
        let to_start: Vec<Uuid> = desired
            .iter()
            .filter(|id| !state.runners().contains_key(id))
            .copied()
            .collect();
        for item_id in to_start {
            let run_context = match context_for(state, item_id) {
                Ok(Some(context)) => context,
                Ok(None) => continue,
                Err(error) => {
                    // Creating the package folder failed. Left swallowed this
                    // was invisible: `SparseFile` then failed to open, the
                    // failure classified transient, and the item re-attempted
                    // once a second forever with nothing anywhere saying why.
                    let landing = self.failure_state(state, &error, item_id, false);
                    state.mutate_item(item_id, |item| item.state = landing);
                    state.failed_attempts_mut().remove(&item_id);
                    changed = true;
                    continue;
                }
            };
            if run_context
                .contexts
                .iter()
                .any(|context| claimed.contains(&context.destination))
            {
                continue;
            }
            for context in &run_context.contexts {
                claimed.insert(context.destination.clone());
            }
            state.mutate_item(item_id, |item| item.state = ItemState::Running);
            let current_tick = state.current_tick;
            state.started_at_tick.insert(item_id, current_tick);
            changed = true;

            let completed_total_bytes = state.completed_ranges_of(item_id).total_bytes();
            state
                .attempt_start_bytes
                .insert(item_id, completed_total_bytes);
            // The one place a byte total may legitimately go backwards: a new
            // task starts over from whatever the sidecar lets it resume at.
            // Seeding the sampler baseline here — rather than letting
            // `record_progress` rebase itself — keeps the delta strictly
            // monotonic everywhere else.
            state.sampled_bytes.insert(item_id, completed_total_bytes);

            let total_segments = allocated_segments
                .get(&item_id)
                .copied()
                .unwrap_or(run_context.segments);
            let per_component = (total_segments / run_context.contexts.len()).max(1);
            let item_page_url = state
                .find_item(item_id)
                .map(|(_, item)| item.source_url.clone());
            let components: Vec<ComponentRun> = run_context
                .contexts
                .iter()
                .map(|context| {
                    self.component_run(state, item_id, &item_page_url, context, per_component)
                })
                .collect();

            let run_id = Uuid::new_v4();
            let cancel = CancellationToken::new();
            let done = JobDone::new();
            let weak = Arc::downgrade(self);
            let job_cancel = cancel.clone();
            let guard = JobDoneGuard(done.clone());
            tokio::spawn(async move {
                let _done = guard;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.run_item(item_id, run_id, job_cancel).await;
            });
            state.runners_mut().insert(
                item_id,
                Runner {
                    components,
                    cancel,
                    done,
                    run_id,
                    retire_intent: RetireIntent::None,
                },
            );
        }

        let resizes: Vec<(Vec<ComponentRun>, usize)> = state
            .runners()
            .iter()
            .filter(|(id, runner)| {
                runner.retire_intent == RetireIntent::None && desired.contains(id)
            })
            .filter_map(|(id, runner)| {
                allocated_segments.get(id).map(|allocated| {
                    let per_component = (allocated / runner.components.len()).max(1);
                    (runner.components.clone(), per_component)
                })
            })
            .collect();

        let in_flight = state
            .runners()
            .values()
            .next()
            .map(|runner| runner.done.clone());

        Applied {
            retiring,
            resizes,
            changed,
            in_flight,
        }
    }

    /// Builds the worker for one component of a run that is starting.
    fn component_run(
        &self,
        state: &EngineState,
        item_id: Uuid,
        item_page_url: &Option<SourceUrl>,
        context: &ComponentContext,
        per_component: usize,
    ) -> ComponentRun {
        let worker = match (&context.origin, &self.wholesale_downloader) {
            (ComponentOrigin::Wholesale { format_selector }, Some(downloader)) => {
                ComponentWorker::Wholesale(Arc::new(WholesaleComponentTask::new(
                    item_id,
                    item_page_url
                        .clone()
                        .unwrap_or_else(|| context.source_url.clone()),
                    format_selector.clone(),
                    context.destination.clone(),
                    downloader.clone(),
                )))
            }
            _ => {
                let mut configuration = DownloadTaskConfiguration::new(
                    per_component,
                    state.settings().min_segment_size_bytes,
                    state.settings().checkpoint_interval_bytes,
                    Some(context.cached_completed.clone()),
                );
                if let Some(format_id) = &context.refreshable_format_id {
                    configuration = configuration.with_refreshable_format_id(format_id.clone());
                }
                ComponentWorker::Segmented(Arc::new(DownloadTask::new(
                    item_id,
                    context.source_url.clone(),
                    context.destination.clone(),
                    self.transport.clone(),
                    configuration,
                )))
            }
        };
        ComponentRun {
            component_index: context.component_index,
            worker,
            destination: context.destination.clone(),
        }
    }
}

/// What `decide_and_apply` leaves for `reconcile()` to do once the lock is
/// released.
struct Applied {
    /// Components of runners just marked `Preempted`, to pause.
    retiring: Vec<ComponentRun>,
    /// Surviving runners' components and their new per-component worker
    /// count.
    resizes: Vec<(Vec<ComponentRun>, usize)>,
    changed: bool,
    /// One runner in flight once the decision was applied, if any.
    in_flight: Option<JobDone>,
}

/// Connection demand for a set of items: their real host and their requested
/// (not yet budget-capped) worker count.
fn connection_demands(state: &EngineState, item_ids: &HashSet<Uuid>) -> Vec<ConnectionDemand> {
    let mut demands = Vec::new();
    for package in state.packages() {
        for item in package
            .items
            .iter()
            .filter(|item| item_ids.contains(&item.id))
        {
            demands.push(ConnectionDemand::new(
                item.id,
                // The host actually connected to — the first component's
                // stream URL, not the grabbed source URL (which for a YouTube
                // item is `youtube.com`, where nothing is downloaded from).
                item.components
                    .first()
                    .and_then(|component| component.url.as_url())
                    .and_then(|url| url.host_str())
                    .unwrap_or(""),
                state.segment_count(item.id),
            ));
        }
    }
    demands
}

/// The package graph as the scheduler should see it right now.
///
/// An item serving retry backoff is presented as disabled, so it drops out of
/// `rank` entirely and its slot goes to the next item by rank rather than
/// being held empty. Doing it here — rather than subtracting from the
/// scheduler's answer afterwards — keeps `Scheduler::desired_running_set` a
/// pure function of its input, which is what makes spec §6 table-testable.
fn schedulable_packages(state: &EngineState) -> Vec<DownloadPackage> {
    let mut held = state.packages().to_vec();
    if state.retry_hold_ticks().is_empty() {
        return held;
    }
    for item in held.iter_mut().flat_map(|package| package.items.iter_mut()) {
        if state.retry_hold_ticks().contains_key(&item.id) {
            item.is_enabled = false;
        }
    }
    held
}

/// Mirrors each running task's probe result onto its item.
///
/// The scheduler reads `is_resumable` straight off `packages`, so the answer
/// has to live there rather than in a side table. This is the earliest point
/// the engine can observe it without blocking the decision: a task reports
/// `None` until its probe lands, and the true value from then on. The
/// `None -> Some(false)` transition is the one that matters — it is what stops
/// a genuinely non-resumable download from being preempted and losing every
/// byte it has — and running this at the head of every `reconcile()` means the
/// very scheduling decision that could preempt it is already looking at the
/// real value.
fn refresh_resumability(state: &mut EngineState) {
    let probed: Vec<(Uuid, usize, bool)> = state
        .runners()
        .iter()
        .filter(|(_, runner)| runner.retire_intent == RetireIntent::None)
        .flat_map(|(id, runner)| {
            runner.components.iter().filter_map(|component| {
                component
                    .probed_supports_ranges()
                    .map(|supports| (*id, component.component_index, supports))
            })
        })
        .collect();
    for (item_id, component_index, supports_ranges) in probed {
        state.mutate_item(item_id, |item| {
            if component_index < item.components.len() {
                item.components[component_index].is_resumable = Some(supports_ranges);
            }
        });
    }
}

struct ComponentContext {
    component_index: usize,
    source_url: SourceUrl,
    destination: PathBuf,
    cached_completed: RangeSet,
    refreshable_format_id: Option<String>,
    origin: ComponentOrigin,
}

struct ItemRunContext {
    contexts: Vec<ComponentContext>,
    segments: usize,
}

/// Resolves where each not-yet-complete component of an item writes, creating
/// the package folder.
///
/// Fails rather than swallowing a `create_dir_all` failure: a read-only volume
/// or a permission problem is not something re-attempting fixes, and hiding it
/// turned it into an undiagnosable retry loop one layer down.
fn context_for(state: &EngineState, item_id: Uuid) -> Result<Option<ItemRunContext>, EngineError> {
    let Some((package, item)) = state.find_item(item_id) else {
        return Ok(None);
    };
    let folder = state.settings().download_folder.join(&package.name);
    std::fs::create_dir_all(&folder).map_err(|error| {
        EngineError::DestinationFolderUnavailable {
            path: folder.to_string_lossy().into_owned(),
            underlying: error.to_string(),
        }
    })?;
    let mut contexts: Vec<ComponentContext> = Vec::new();
    for (index, component) in item
        .components
        .iter()
        .enumerate()
        .filter(|(_, component)| !component.is_complete())
    {
        let refreshable_format_id = match &component.origin {
            ComponentOrigin::Resolved { format_id } => Some(format_id.clone()),
            _ => None,
        };
        // A wholesale component never resumes — it re-runs yt-dlp from zero —
        // so its cached progress is always discarded.
        let cached_completed = if matches!(component.origin, ComponentOrigin::Wholesale { .. }) {
            RangeSet::new()
        } else {
            component.completed.clone()
        };
        contexts.push(ComponentContext {
            component_index: index,
            source_url: component.url.clone(),
            destination: folder.join(&component.part_filename),
            cached_completed,
            refreshable_format_id,
            origin: component.origin.clone(),
        });
    }
    // Every component already complete → still hand back one context for
    // component 0 so the run finalizes/assembles rather than stalling.
    // (`is_complete` on a component needs a known size; an unprobed component
    // is not complete, so this only bites a truly finished item that lost its
    // runner.)
    if contexts.is_empty() {
        let first = &item.components[0];
        contexts.push(ComponentContext {
            component_index: 0,
            source_url: first.url.clone(),
            destination: folder.join(&first.part_filename),
            cached_completed: first.completed.clone(),
            refreshable_format_id: None,
            origin: first.origin.clone(),
        });
    }
    Ok(Some(ItemRunContext {
        contexts,
        segments: state.segment_count(item_id),
    }))
}
