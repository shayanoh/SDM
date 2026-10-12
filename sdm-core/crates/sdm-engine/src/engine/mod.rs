//! Owns the package list, the running `DownloadTask`s, and the scheduler.
//! Port of `DownloadEngine.swift`.
//!
//! `tick()` is driven by the app once per second rather than by an internal
//! timer, so the engine is fully deterministic under test. It is also what
//! picks up newly runnable work: a finished download does not reschedule from
//! inside its own completion path, because a transient failure would then
//! re-attempt itself with no gap at all. Re-attempts are governed by
//! `RetryPolicy` — a per-item attempt counter, tick-counted backoff before an
//! item may be re-desired, and a terminal `Failed { reason }` once
//! `max_attempts` consecutive attempts have failed.
//!
//! # From an actor to a mutex
//!
//! Swift's `DownloadEngine` is an `actor`: between two `await`s its state
//! cannot change, and after each `await` it can. Here all of that state sits
//! in [`EngineState`](state::EngineState) behind one `std::sync::Mutex`, which
//! is **never held across an `.await`**. So every Swift stretch without a
//! suspension becomes one lock scope, and after every `.await` the code
//! re-reads state instead of trusting what it saw before the suspension.
//! Anything that suspends in Swift but is synchronous here (the task getters,
//! `refresh_resumability`) simply runs inside the scope that needs it.
//!
//! The lock order is engine, then a task's own lock; a task never calls back
//! into the engine. Cancellation is an explicit `CancellationToken` per
//! runner; nothing is ever `abort`ed.
//!
//! # Swift's comments, kept for the types and `reconcile()`
//!
//! `Runner`: *One in-flight item: every component's task plus the single job
//! awaiting them and running assembly.*
//!
//! `RetireIntent`: *Why a runner is being torn down, decided at the moment it
//! is marked retiring and consulted later, once `run()`'s job actually
//! finishes, to pick the item's landing state. The two outcomes are genuinely
//! different: a preempted item must come back `.queued` — it is still
//! desired, just outranked for a slot right now — while a user-stopped item
//! must land `.stopped` and stay there until explicitly started again.*
//!
//! The Swift doc comment above its `Runner` (kept verbatim, though it sits
//! under `ComponentWorker` in the source): *A preempted or user-stopped
//! runner is marked retiring and kept here until its job has actually
//! unwound. That is what stops a second `DownloadTask` from being pointed at
//! a destination file the first one has not finished with. It does not bound
//! `runners.count` by `maxConcurrent`. A retiring runner is still reported to
//! the scheduler in `runningNow`, but `runningNow` only feeds passes 1 and 2,
//! and a resumable (or not yet probed) item is eligible for neither — pass 1
//! covers only `isResumable == false`, and pass 2 is empty while hysteresis
//! is unwired. So a retiring resumable item can be outranked, leaving
//! `runners` transiently holding the retiring runner plus a full complement
//! of new ones. That is benign: a retiring runner has already been paused, so
//! its worker target is zero and its descriptor is closed, and it issues no
//! further requests. What is bounded at all times is the number of items in
//! state `.running`, since preemption sets `.queued` synchronously.*
//!
//! `reconcile()`: *Applies the scheduler's desired running set: starts what
//! should run, retires what should not. The desired set is computed and
//! applied to `runners` without any suspension in between, so two overlapping
//! calls can never both decide to start the same item. `startedRecently` is
//! derived from `startedAtTick` and the tick-counted hysteresis window (spec
//! §6.4), rather than a wall clock — see `currentTick`. `refreshResumability()`
//! runs before that block, and does suspend. That is safe for a different
//! reason than the block itself: a call that suspends there and finds another
//! `reconcile()` has run to completion in the meantime simply recomputes from
//! the state that call left behind and applies it, which is indistinguishable
//! from the two having run in sequence. What must not happen — and does not —
//! is a suspension between deciding the desired set and acting on it.* Here
//! `refresh_resumability` does not suspend at all (the task getters are
//! synchronous), so it shares the one lock scope with the decision.

mod lifecycle;
mod organize;
mod persist;
mod reconcile;
mod run;
mod settings;
mod snapshot;
mod state;
mod tick;

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use sdm_core::{LinkResolver, RangeSet, WholesaleDownloader, WholesaleError};
use thiserror::Error;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::download_task::{ComponentTelemetry, DownloadError, DownloadTask};
use crate::http_transport::HttpTransport;
use crate::muxer::Muxer;
use crate::retry_policy::RetryPolicy;
use crate::state_store::StateStore;
use crate::wholesale_component_task::WholesaleComponentTask;

pub use organize::{SystemTrash, Trash};
pub use settings::EngineSettings;
use state::EngineState;

/// Failures the engine itself produces, as opposed to the transport or the
/// per-download task. Routed through `RetryPolicy::classify` like any other
/// error so they reach a terminal `Failed { reason }` instead of a retry loop.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    /// The per-package output folder could not be created — permissions, a
    /// read-only volume, a file sitting where the folder should be.
    #[error("could not create the folder {path}: {underlying}")]
    DestinationFolderUnavailable { path: String, underlying: String },
}

/// Owns the package list, the running tasks and the scheduler. Cloning is not
/// offered: a `DownloadEngine` is the single handle, as Swift's actor
/// reference is, and its jobs hold only a `Weak` to the inside.
pub struct DownloadEngine {
    inner: Arc<Inner>,
}

/// Everything behind the handle: the injected collaborators, which never
/// change, and the one mutex over the mutable state.
pub(crate) struct Inner {
    transport: Arc<dyn HttpTransport>,
    state_store: Arc<dyn StateStore>,
    /// Read by the URL-refresh branch of `run_component`, which Task 18 adds.
    #[allow(dead_code)]
    resolver: Option<Arc<dyn LinkResolver>>,
    muxer: Option<Arc<dyn Muxer>>,
    wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    retry_policy: RetryPolicy,
    /// Where removed and reset files go. Set once, by `with_trash`, before the
    /// engine is shared.
    trash: Arc<dyn Trash>,
    /// Spec §6.4: "An item started within the last ~5 s is not preempted,"
    /// expressed in ticks at `AppTiming::TICKS_PER_SECOND` rather than a raw
    /// tick count, so raising the heartbeat rate does not shrink the window.
    hysteresis_window_ticks: usize,
    /// Orders `persist()` calls. Swift enqueues each `stateStore.save` on the
    /// store's actor in the order the engine actor issued it; here the
    /// snapshot is taken and saved while holding this gate, so an older
    /// snapshot can never land after a newer one. Not engine state, which is
    /// why it is allowed to be held across an `.await` (an async gate, not
    /// the state mutex).
    persist_gate: tokio::sync::Semaphore,
    state: Mutex<EngineState>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, EngineState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// MARK: - Runners

/// One component's worker: a segmented HTTP task, or a wholesale (yt-dlp as
/// the downloader) one. Parent spec §7.1 /
/// `2026-09-03-multi-site-resolver-design.md` §6.7.
#[derive(Clone)]
pub(crate) enum ComponentWorker {
    Segmented(Arc<DownloadTask>),
    Wholesale(Arc<WholesaleComponentTask>),
}

/// A component's `start()` failure in one type that still lets
/// `RetryPolicy::classify` see the original error (`as_error`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComponentFailure {
    /// Swift's `CancellationError`: a wholesale component that was paused.
    Cancelled,
    Download(DownloadError),
    Wholesale(WholesaleError),
}

/// Stand-in for Swift's `CancellationError`, so `classify` and `describe` have
/// a concrete error to downcast.
#[derive(Debug)]
pub(crate) struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cancelled")
    }
}

impl Error for Cancelled {}

impl ComponentFailure {
    /// The underlying error, unwrapped for `downcast_ref`.
    pub(crate) fn as_error(&self) -> &(dyn Error + 'static) {
        match self {
            Self::Cancelled => &Cancelled,
            Self::Download(error) => error,
            Self::Wholesale(error) => error,
        }
    }
}

/// One component of an in-flight item. A generic HTTP item has one; a muxed
/// YouTube item has two; an HLS/DASH item has one wholesale worker.
#[derive(Clone)]
pub(crate) struct ComponentRun {
    /// Index into `DownloadItem::components` — stable across the run.
    pub(crate) component_index: usize,
    pub(crate) worker: ComponentWorker,
    pub(crate) destination: PathBuf,
}

impl ComponentRun {
    /// The `DownloadTask`, when this is a segmented component (the `403`
    /// refresh path only applies there). Read by Task 18's refresh branch.
    #[allow(dead_code)]
    pub(crate) fn segmented_task(&self) -> Option<&Arc<DownloadTask>> {
        match &self.worker {
            ComponentWorker::Segmented(task) => Some(task),
            ComponentWorker::Wholesale(_) => None,
        }
    }

    pub(crate) async fn start(&self) -> Result<PathBuf, ComponentFailure> {
        match &self.worker {
            ComponentWorker::Segmented(task) => {
                task.start().await.map_err(ComponentFailure::Download)
            }
            ComponentWorker::Wholesale(task) => task.start().await.map_err(|error| match error {
                // `Cancelled` is the pause / stop signal, which Swift's
                // `runComponent` turns into a `CancellationError`.
                WholesaleError::Cancelled => ComponentFailure::Cancelled,
                other => ComponentFailure::Wholesale(other),
            }),
        }
    }

    /// Stops the component. A wholesale pause returns only once its downloader
    /// call has really stopped.
    pub(crate) async fn pause(&self) {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.pause(),
            ComponentWorker::Wholesale(task) => task.pause().await,
        }
    }

    pub(crate) fn completed_ranges(&self) -> RangeSet {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.completed_ranges(),
            ComponentWorker::Wholesale(task) => task.completed_ranges(),
        }
    }

    pub(crate) fn telemetry(&self) -> ComponentTelemetry {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.telemetry(),
            ComponentWorker::Wholesale(task) => task.telemetry(),
        }
    }

    pub(crate) fn expected_total_bytes(&self) -> Option<i64> {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.expected_total_bytes(),
            ComponentWorker::Wholesale(task) => task.expected_total_bytes(),
        }
    }

    pub(crate) fn probed_supports_ranges(&self) -> Option<bool> {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.probed_supports_ranges(),
            ComponentWorker::Wholesale(task) => task.probed_supports_ranges(),
        }
    }

    pub(crate) fn last_checkpoint_failure(&self) -> Option<String> {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.last_checkpoint_failure(),
            ComponentWorker::Wholesale(task) => task.last_checkpoint_failure(),
        }
    }

    pub(crate) fn checkpoint_tick(&self) {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.checkpoint_tick(),
            ComponentWorker::Wholesale(task) => task.checkpoint_tick(),
        }
    }

    pub(crate) fn set_worker_count(&self, count: usize) {
        match &self.worker {
            ComponentWorker::Segmented(task) => task.set_worker_count(count),
            ComponentWorker::Wholesale(task) => task.set_worker_count(count),
        }
    }
}

/// Why a runner is being torn down. See the module docs for Swift's comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetireIntent {
    None,
    Preempted,
    UserStopped,
}

/// Lets any number of callers wait for a runner's job to finish — Swift's
/// `await runner.job.value`, which a `JoinHandle` (awaitable once) cannot do.
#[derive(Clone)]
pub(crate) struct JobDone {
    sender: Arc<watch::Sender<bool>>,
}

impl JobDone {
    fn new() -> Self {
        Self {
            sender: Arc::new(watch::channel(false).0),
        }
    }

    fn finish(&self) {
        self.sender.send_replace(true);
    }

    /// Returns once the job has finished (immediately if it already has).
    pub(crate) async fn wait(&self) {
        let mut receiver = self.sender.subscribe();
        // The sender lives in `self`, so the channel cannot close under us.
        let _ = receiver.wait_for(|finished| *finished).await;
    }
}

/// Marks the job finished when its task ends, however it ends.
struct JobDoneGuard(JobDone);

impl Drop for JobDoneGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// One in-flight item: every component's task plus the single job awaiting
/// them and running assembly.
#[derive(Clone)]
pub(crate) struct Runner {
    pub(crate) components: Vec<ComponentRun>,
    /// Cancelling it is Swift's `job.cancel()`: it reaches the mux, which is
    /// the one step of the job that is not stopped by pausing the components.
    pub(crate) cancel: CancellationToken,
    /// Signalled when the job has finished and unwound.
    pub(crate) done: JobDone,
    /// Identifies this specific run, so a superseded job's late
    /// `finish_item` cannot clobber its replacement.
    pub(crate) run_id: Uuid,
    pub(crate) retire_intent: RetireIntent,
}

impl Runner {
    pub(crate) fn all_destinations(&self) -> impl Iterator<Item = &PathBuf> {
        self.components
            .iter()
            .map(|component| &component.destination)
    }

    pub(crate) async fn pause_all_components(&self) {
        for component in &self.components {
            component.pause().await;
        }
    }
}

impl DownloadEngine {
    /// # Panics
    ///
    /// Panics if `settings` breaks one of its preconditions (see
    /// [`EngineSettings::new`]).
    pub fn new(
        transport: Arc<dyn HttpTransport>,
        state_store: Arc<dyn StateStore>,
        settings: EngineSettings,
        retry_policy: RetryPolicy,
        resolver: Option<Arc<dyn LinkResolver>>,
        muxer: Option<Arc<dyn Muxer>>,
        wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    ) -> Self {
        settings.validate();
        Self {
            inner: Arc::new(Inner {
                transport,
                state_store,
                resolver,
                muxer,
                wholesale_downloader,
                retry_policy,
                trash: Arc::new(SystemTrash),
                hysteresis_window_ticks: sdm_core::AppTiming::TICKS_PER_SECOND * 5,
                persist_gate: tokio::sync::Semaphore::new(1),
                state: Mutex::new(EngineState::new(settings)),
            }),
        }
    }

    /// Replaces where removed and reset files go (the user's Trash by
    /// default). Tests inject a [`Trash`] that moves them into a scratch
    /// directory instead.
    ///
    /// # Panics
    ///
    /// Panics if the engine is already shared — call it straight after `new`.
    #[must_use]
    pub fn with_trash(mut self, trash: Arc<dyn Trash>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("with_trash must be called before the engine is shared")
            .trash = trash;
        self
    }
}
