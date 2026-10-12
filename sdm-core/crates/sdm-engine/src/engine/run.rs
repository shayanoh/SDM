//! Running an item: its components in parallel, assembly, and landing.

use std::error::Error;
use std::sync::Arc;

use sdm_core::{Assembly, DownloadItem, ItemState, MediaContainer, RangeSet, WholesaleError};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::state::EngineState;
use super::{Cancelled, ComponentFailure, ComponentRun, Inner, RetireIntent};
use crate::download_task::DownloadError;
use crate::http_transport::TransportError;
use crate::muxer::MuxError;
use crate::resume_sidecar::ResumeSidecar;
use crate::retry_policy::FailureKind;
use crate::sparse_file::SparseFileError;

/// How the assembly step ended.
enum Assembled {
    /// The item lands in this state.
    Landed(ItemState),
    /// The step was cut short by the job being cancelled — the item was
    /// paused, preempted, or the engine is shutting down — rather than
    /// failing, so it must not be recorded as a failure.
    Interrupted,
}

impl Inner {
    /// Runs every component task of an item in parallel, then assembles.
    /// Replaces the single-task `run`; a one-component item is the N=1 case.
    pub(super) async fn run_item(
        self: &Arc<Self>,
        item_id: Uuid,
        run_id: Uuid,
        cancel: CancellationToken,
    ) {
        let Some(components) = self
            .lock()
            .runners()
            .get(&item_id)
            .map(|runner| runner.components.clone())
        else {
            return;
        };

        // Fan out. Any throw is the first error we keep, and the siblings are
        // paused. (A component failing with `UrlExpired` will be refreshed and
        // restarted in place once Task 18 adds that branch to
        // `run_component`.)
        let mut group = JoinSet::new();
        for component in components {
            let inner = Arc::clone(self);
            group.spawn(async move { inner.run_component(item_id, component).await });
        }
        let mut first_error: Option<ComponentFailure> = None;
        while let Some(joined) = group.join_next().await {
            let failure = match joined {
                Ok(failure) => failure,
                Err(join_error) => {
                    if join_error.is_panic() {
                        std::panic::resume_unwind(join_error.into_panic());
                    }
                    // Nothing aborts these tasks, so this is unreachable; a
                    // component that vanished must not read as success.
                    Some(ComponentFailure::Download(
                        DownloadError::IncompleteAfterWorkersFinished,
                    ))
                }
            };
            if let Some(failure) = failure
                && first_error.is_none()
            {
                first_error = Some(failure);
                // Stop the siblings — their partial bytes stay on disk.
                let live = self
                    .lock()
                    .runners()
                    .get(&item_id)
                    .map(|runner| runner.components.clone());
                for sibling in live.iter().flatten() {
                    sibling.pause().await;
                }
            }
        }

        let retire_intent = self
            .lock()
            .runners()
            .get(&item_id)
            .map_or(RetireIntent::None, |runner| runner.retire_intent);
        let landing: Option<ItemState> = match (retire_intent, first_error) {
            (RetireIntent::Preempted, _) => Some(ItemState::Queued),
            (RetireIntent::UserStopped, _) => None,
            (RetireIntent::None, Some(failure)) => {
                // Ask the *live* runners how far this attempt got —
                // `item.completed` is not written back until `finish_item()`
                // below, so reading it here only ever sees the previous
                // attempt's bytes (i.e. `attempt_start_bytes`), and a
                // resumable download that downloaded real bytes this attempt
                // before dropping would never clear its failure counter. That
                // let five interrupted-but-progressing attempts add up to a
                // terminal `Failed`.
                let live = self
                    .lock()
                    .runners()
                    .get(&item_id)
                    .map(|runner| runner.components.clone());
                let attempt_completed: i64 = live
                    .iter()
                    .flatten()
                    .map(|component| component.completed_ranges().total_bytes())
                    .sum();
                let mut state = self.lock();
                let started_from = state
                    .attempt_start_bytes
                    .get(&item_id)
                    .copied()
                    .unwrap_or(0);
                let progressed = attempt_completed > started_from;
                Some(self.failure_state(&mut state, failure.as_error(), item_id, progressed))
            }
            (RetireIntent::None, None) => match self.assemble(item_id, &cancel).await {
                Assembled::Landed(landed) => {
                    if landed == ItemState::Completed {
                        let mut state = self.lock();
                        state.failed_attempts_mut().remove(&item_id);
                        state.retry_hold_ticks_mut().remove(&item_id);
                        // Just written, so not missing — whatever an earlier
                        // check of a previous copy of this file concluded.
                        state.missing_files_mut().remove(&item_id);
                    }
                    Some(landed)
                }
                // Paused or stopped while the mux was running. Read the
                // intent again — it was set after the read above — and land
                // as a pause would have: queued if preempted, untouched if
                // the user stopped it (or the engine is shutting down, where
                // `persist()` squashes the in-memory state anyway).
                Assembled::Interrupted => {
                    let intent = self
                        .lock()
                        .runners()
                        .get(&item_id)
                        .map_or(RetireIntent::None, |runner| runner.retire_intent);
                    (intent == RetireIntent::Preempted).then_some(ItemState::Queued)
                }
            },
        };

        self.finish_item(item_id, run_id, landing);
        self.persist().await;
    }

    /// Runs one component's task to completion. Returns the failure, or
    /// `None` on success.
    ///
    /// A wholesale component re-resolves internally on every run. A
    /// `Cancelled` from one reaches `run_item` as the pause/stop signal via
    /// the retire-intent branch.
    ///
    /// The `403`/`410` URL-refresh branch for `Resolved` components (the
    /// resolver, then a replacement `DownloadTask` swapped into the runner)
    /// is Task 18's: until then a `UrlExpired` is returned as the error, which
    /// `RetryPolicy` classifies as transient.
    async fn run_component(
        self: &Arc<Self>,
        _item_id: Uuid,
        component: ComponentRun,
    ) -> Option<ComponentFailure> {
        component.start().await.err()
    }

    /// Assembly step after every component completed. `Assembly::None` → each
    /// task already finalized its own file, nothing to do. `Mux` → run `ffmpeg
    /// -c copy` over the finalized parts into the output container, then
    /// delete the parts. Parent spec §7.2.
    async fn assemble(&self, item_id: Uuid, cancel: &CancellationToken) -> Assembled {
        let (item, package_name, folder) = {
            let state = self.lock();
            let Some((package, item)) = state.find_item(item_id) else {
                return Assembled::Landed(ItemState::Completed);
            };
            (
                item.clone(),
                package.name.clone(),
                state.settings().download_folder.join(&package.name),
            )
        };
        if item.assembly == Assembly::Mux {
            return self.run_mux(&item, &package_name, cancel).await;
        }
        // No mux, but a single component whose finalized part file is not
        // already the output name (a progressive YouTube stream lands as
        // `<title> [id].f18.mp4`, the item's output is `<title> [id].mp4`).
        // Rename it so the item's destination matches what is on disk.
        if item.components.len() == 1 && item.components[0].part_filename != item.output_filename {
            let from = folder.join(&item.components[0].part_filename);
            let to = folder.join(&item.output_filename);
            if from.exists() {
                let _ = std::fs::remove_file(&to);
                if let Err(error) = std::fs::rename(&from, &to) {
                    return Assembled::Landed(ItemState::Failed {
                        reason: format!("Could not name the file: {error}"),
                    });
                }
            }
            ResumeSidecar::remove(&ResumeSidecar::path_for(&from));
        }
        Assembled::Landed(ItemState::Completed)
    }

    /// Runs the mux for a `Mux` item whose components are all downloaded.
    /// Shared by `assemble()` and the retry-mux action (Task 17).
    ///
    /// A mux that ends because `cancel` fired — the item was paused,
    /// preempted or the engine is shutting down — is `Interrupted`, not a
    /// failure. (Swift's `catch` records any mux error, cancellation
    /// included, as `.failed("mux failed: CancellationError()")`, which
    /// overrides a user's Stop with a failure; the brief for this port rules
    /// that out.)
    async fn run_mux(
        &self,
        item: &DownloadItem,
        package_name: &str,
        cancel: &CancellationToken,
    ) -> Assembled {
        let Some(muxer) = &self.muxer else {
            return Assembled::Landed(ItemState::Failed {
                reason: "ffmpeg is not configured".to_string(),
            });
        };
        if item.components.len() != 2 {
            return Assembled::Landed(ItemState::Failed {
                reason: "a muxed item needs exactly two components".to_string(),
            });
        }
        let folder = self.lock().settings().download_folder.join(package_name);
        let video_part = folder.join(&item.components[0].part_filename);
        let audio_part = folder.join(&item.components[1].part_filename);
        let output = folder.join(&item.output_filename);
        let container = MediaContainer::from_file_extension(
            &std::path::Path::new(&item.output_filename)
                .extension()
                .map(|extension| extension.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );

        self.lock().assembling_mut().insert(item.id);
        let result = muxer
            .mux(&video_part, &audio_part, &output, &container, cancel)
            .await;
        self.lock().assembling_mut().remove(&item.id);

        if let Err(error) = result {
            if cancel.is_cancelled() {
                return Assembled::Interrupted;
            }
            let tail = match &error {
                MuxError::FfmpegFailed { stderr_tail } => stderr_tail.clone(),
                MuxError::FfmpegMissing => "ffmpegMissing".to_string(),
                MuxError::TimedOut => "timedOut".to_string(),
                MuxError::Process(inner) => format!("{inner:?}"),
            };
            return Assembled::Landed(ItemState::Failed {
                reason: format!("mux failed: {tail}"),
            });
        }
        let _ = std::fs::remove_file(&video_part);
        let _ = std::fs::remove_file(&audio_part);
        for component in &item.components {
            ResumeSidecar::remove(&ResumeSidecar::path_for(
                &folder.join(&component.part_filename),
            ));
        }
        Assembled::Landed(ItemState::Completed)
    }

    /// Retires a finished runner, writing each component's final bytes back
    /// onto `DownloadItem::components[k]` so a preempted or failed item
    /// resumes rather than restarts.
    ///
    /// One lock scope. Swift suspends while it reads the tasks and so has to
    /// re-check that its run was not superseded before it removes the runner;
    /// here the check at the top covers the whole scope.
    fn finish_item(&self, item_id: Uuid, run_id: Uuid, landing: Option<ItemState>) {
        let mut state = self.lock();
        let Some(runner) = state
            .runners()
            .get(&item_id)
            .filter(|runner| runner.run_id == run_id)
            .cloned()
        else {
            return;
        };

        struct Final {
            index: usize,
            completed: RangeSet,
            total_bytes: Option<i64>,
            is_resumable: Option<bool>,
        }
        let mut total_completed: i64 = 0;
        let mut per_component: Vec<Final> = Vec::new();
        for component in &runner.components {
            let completed = component.completed_ranges();
            let total_bytes = component.expected_total_bytes();
            let is_resumable = component.probed_supports_ranges();
            if let Some(failure) = component.last_checkpoint_failure() {
                state.checkpoint_failures_mut().insert(item_id, failure);
            }
            total_completed += completed.total_bytes();
            per_component.push(Final {
                index: component.component_index,
                completed,
                total_bytes,
                is_resumable,
            });
        }

        state.record_progress(total_completed, item_id);

        state.runners_mut().remove(&item_id);
        state.started_at_tick.remove(&item_id);

        let completed_landing = landing == Some(ItemState::Completed);
        state.mutate_item(item_id, |item| {
            for entry in per_component {
                if entry.index >= item.components.len() {
                    continue;
                }
                let component = &mut item.components[entry.index];
                // Same non-resumable-restart display rule as before, per
                // component: a confirmed non-resumable component that is not
                // landing `Completed` shows zero rather than a partial that
                // its next attempt will discard.
                component.completed = if entry.is_resumable == Some(false) && !completed_landing {
                    RangeSet::new()
                } else {
                    entry.completed
                };
                if let Some(total_bytes) = entry.total_bytes {
                    component.total_bytes = Some(total_bytes);
                }
                if let Some(is_resumable) = entry.is_resumable {
                    component.is_resumable = Some(is_resumable);
                }
            }
            if let Some(landing) = landing {
                item.state = landing;
            }
        });
    }

    // MARK: - Failure handling

    /// Turns a thrown error into the item's next state, applying spec §6.4's
    /// backoff and attempt cap.
    ///
    /// Before this existed, `run()` returned every transient failure to
    /// `Queued` and `tick()` re-desired it one second later, forever: against
    /// an origin that 500s, truncates, or drops the connection that is an
    /// unbounded request storm at one attempt per second per item.
    /// `run_item()` and `reconcile()` both route through here so a folder that
    /// cannot be created is capped the same way a hostile origin is.
    ///
    /// A permanent failure is terminal immediately. A transient one holds the
    /// item out of the desired running set for `RetryPolicy::delay_for_attempt`
    /// — counted in heartbeat ticks, no clock — and becomes terminal once
    /// `max_attempts` consecutive attempts have failed. An attempt that moved
    /// real bytes clears the counter first: a download that is genuinely
    /// progressing, however unreliably, should not exhaust a budget meant for
    /// one that cannot start at all.
    pub(super) fn failure_state(
        &self,
        state: &mut EngineState,
        error: &(dyn Error + 'static),
        item_id: Uuid,
        made_progress: bool,
    ) -> ItemState {
        if let FailureKind::Permanent { reason } = self.retry_policy.classify(error) {
            state.failed_attempts_mut().remove(&item_id);
            state.retry_hold_ticks_mut().remove(&item_id);
            state.last_failure_mut().remove(&item_id);
            return ItemState::Failed { reason };
        }

        if made_progress {
            state.failed_attempts_mut().remove(&item_id);
        }
        let attempt = state.failed_attempts().get(&item_id).copied().unwrap_or(0) + 1;
        state.failed_attempts_mut().insert(item_id, attempt);
        state.last_failure_mut().insert(item_id, describe(error));

        if attempt >= self.retry_policy.max_attempts {
            state.failed_attempts_mut().remove(&item_id);
            state.retry_hold_ticks_mut().remove(&item_id);
            state.last_failure_mut().remove(&item_id);
            return ItemState::Failed {
                reason: format!("Gave up after {attempt} attempts: {}", describe(error)),
            };
        }

        // `delay_for_attempt` is seconds; scale by the heartbeat rate to get
        // ticks. At least one tick, so a sub-second backoff still costs a
        // beat rather than re-attempting immediately.
        let seconds = self.retry_policy.delay_for_attempt(attempt - 1).as_secs() as usize;
        state.retry_hold_ticks_mut().insert(
            item_id,
            (seconds * sdm_core::AppTiming::TICKS_PER_SECOND).max(1),
        );
        ItemState::Queued
    }
}

/// The user-facing text for a transient failure, as Swift's `describe` makes
/// it: a `DownloadError` or `TransportError` prints as its Swift case
/// (`serverError(status: 500)`), anything else as the system's
/// `localizedDescription`. The strings below are what Swift produces for each
/// case, including the generic "operation couldn't be completed" text it
/// gives errors that are not `LocalizedError` (the trailing number is the
/// case's index in its Swift enum).
pub(super) fn describe(error: &(dyn Error + 'static)) -> String {
    if let Some(download) = error.downcast_ref::<DownloadError>() {
        return match download {
            DownloadError::UnknownSize => "unknownSize".to_string(),
            DownloadError::ServerError { status } => format!("serverError(status: {status})"),
            DownloadError::UrlExpired { format_id } => {
                format!("urlExpired(formatID: {format_id:?})")
            }
            DownloadError::RefreshFailed { reason } => {
                format!("refreshFailed(reason: {reason:?})")
            }
            DownloadError::IncompleteAfterWorkersFinished => {
                "incompleteAfterWorkersFinished".to_string()
            }
            DownloadError::TruncatedResponse { expected, received } => {
                format!("truncatedResponse(expected: {expected}, received: {received})")
            }
            // Swift lets the underlying error propagate out of `start()`.
            DownloadError::Transport(inner) => describe(inner),
            DownloadError::File(inner) => describe(inner),
            DownloadError::Move { message, .. } => message.clone(),
            DownloadError::InvalidUrl { .. } => download.to_string(),
        };
    }
    if let Some(transport) = error.downcast_ref::<TransportError>() {
        return match transport {
            TransportError::ConnectionDropped => "connectionDropped".to_string(),
            TransportError::Http { status } => format!("http(status: {status})"),
            TransportError::MalformedResponse => "malformedResponse".to_string(),
            // The raw `URLError` Swift lets through; its description is the text.
            TransportError::Network { description } => description.clone(),
        };
    }
    if error.is::<Cancelled>() {
        return not_completed("Swift.CancellationError", 1);
    }
    if let Some(wholesale) = error.downcast_ref::<WholesaleError>() {
        let index = match wholesale {
            WholesaleError::BinaryMissing => 0,
            WholesaleError::Cancelled => 1,
            WholesaleError::AuthRequired => 2,
            WholesaleError::Unavailable => 3,
            WholesaleError::Failed { .. } => 4,
        };
        return not_completed("SDMCore.WholesaleError", index);
    }
    if let Some(file) = error.downcast_ref::<SparseFileError>() {
        let index = match file {
            SparseFileError::CouldNotOpen { .. } => 0,
            SparseFileError::WriteFailed { .. } => 1,
            SparseFileError::TruncateFailed { .. } => 2,
        };
        return not_completed("SDMEngine.SparseFile.FileError", index);
    }
    error.to_string()
}

fn not_completed(domain: &str, code: usize) -> String {
    format!("The operation couldn\u{2019}t be completed. ({domain} error {code}.)")
}
