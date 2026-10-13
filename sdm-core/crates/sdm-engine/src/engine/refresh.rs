//! The silent URL refresh: when a resolved stream's signed URL expires
//! mid-download (`403`/`410`), ask the resolver for a fresh one and restart the
//! component in place against its existing bytes. Parent spec §7.3.
//!
//! In Swift this is a loop inside one actor method, `runComponent`. Each
//! stretch of that loop without an `await` is one lock scope here, and after
//! the one suspension that can change the world — the resolver call — the
//! runner is read again from state and checked to still be this run, not
//! retiring and not cancelled before anything is installed. (Swift needs no
//! such check: a stopped item's refresh just installs a task nobody starts.)
//! A refresh that comes back after the item was stopped, removed or preempted
//! installs nothing and is an interruption, not a failure.
//!
//! The replacement task is swapped into the runner in the same lock scope as
//! that check, so a Stop that lands before the new task starts finds the new
//! task in the runner and pauses *it* (a pause recorded before `start()`
//! makes `start()` return at once).

use std::path::Path;
use std::sync::Arc;

use sdm_core::{ComponentOrigin, FileComponent, LinkResolver, ResolveError, SourceUrl};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{ComponentFailure, ComponentWorker, Inner, RetireIntent};
use crate::download_task::{DownloadError, DownloadTask, DownloadTaskConfiguration};

/// What the first lock scope after an expiry decided.
enum Expiry {
    /// Stop with this failure.
    Fail(ComponentFailure),
    /// Ask `resolver` for a fresh URL for `format_id`.
    Refresh {
        resolver: Arc<dyn LinkResolver>,
        source_url: SourceUrl,
        component: Box<FileComponent>,
    },
}

impl Inner {
    /// Runs a segmented component's task, refreshing and restarting it each
    /// time it reports an expired URL. Returns the failure, or `None` on
    /// success.
    pub(super) async fn run_segmented_component(
        self: &Arc<Self>,
        item_id: Uuid,
        run_id: Uuid,
        cancel: &CancellationToken,
        component_index: usize,
        destination: &Path,
        task: Arc<DownloadTask>,
    ) -> Option<ComponentFailure> {
        let mut running = task;
        loop {
            let format_id = match running.start().await {
                Ok(_) => return None,
                Err(DownloadError::UrlExpired { format_id }) => format_id,
                Err(error) => return Some(ComponentFailure::Download(error)),
            };

            // One lock scope: Swift's stretch from the catch to `resolver.refresh`.
            let (resolver, source_url, component) =
                match self.begin_refresh(item_id, run_id, cancel, component_index) {
                    Expiry::Fail(failure) => return Some(failure),
                    Expiry::Refresh {
                        resolver,
                        source_url,
                        component,
                    } => (resolver, source_url, component),
                };

            let refreshed = match resolver.refresh(&source_url, &format_id, cancel).await {
                Ok(refreshed) => refreshed,
                // The token fired: the item was stopped, preempted or the
                // engine shut down. Not a failure.
                Err(_) if cancel.is_cancelled() => return Some(ComponentFailure::Cancelled),
                Err(ResolveError::Cancelled) => return Some(ComponentFailure::Cancelled),
                Err(error) => {
                    return Some(refresh_failed(format!(
                        "Could not refresh the URL: {}",
                        describe_resolve_error(&error)
                    )));
                }
            };

            // One lock scope: Swift's stretch from the refresh result to the
            // restart. Re-read the runner first; the resolver call suspended.
            let mut state = self.lock();
            let still_this_run = !cancel.is_cancelled()
                && state.runners().get(&item_id).is_some_and(|runner| {
                    runner.run_id == run_id && runner.retire_intent == RetireIntent::None
                });
            if !still_this_run {
                return Some(ComponentFailure::Cancelled);
            }
            if refreshed.format_id != format_id {
                return Some(refresh_failed("The video's format changed".to_string()));
            }
            if let (Some(known), Some(fresh)) = (component.total_bytes, refreshed.filesize)
                && fresh != known
            {
                return Some(refresh_failed("The video's size changed".to_string()));
            }

            let fresh_url = refreshed.url;
            state.mutate_item(item_id, |item| {
                if component_index < item.components.len() {
                    item.components[component_index].url = fresh_url.clone();
                }
            });
            let component_count = state
                .runners()
                .get(&item_id)
                .map_or(1, |runner| runner.components.len());
            let per_component = (state.segment_count(item_id) / component_count).max(1);
            let configuration = DownloadTaskConfiguration::new(
                per_component,
                state.settings().min_segment_size_bytes,
                state.settings().checkpoint_interval_bytes,
                Some(component.completed.clone()),
            )
            .with_refreshable_format_id(format_id.clone());
            let new_task = Arc::new(DownloadTask::new(
                item_id,
                fresh_url,
                destination.to_path_buf(),
                Arc::clone(&self.transport),
                configuration,
            ));
            if let Some(slot) = state.runners_mut().get_mut(&item_id).and_then(|runner| {
                runner
                    .components
                    .iter_mut()
                    .find(|run| run.component_index == component_index)
            }) {
                slot.worker = ComponentWorker::Segmented(Arc::clone(&new_task));
            }
            drop(state);
            running = new_task;
        }
    }

    /// The first stretch after an expiry: decides whether a refresh is
    /// possible and charges it one attempt against the retry cap, so a
    /// genuinely broken video still terminates.
    fn begin_refresh(
        &self,
        item_id: Uuid,
        run_id: Uuid,
        cancel: &CancellationToken,
        component_index: usize,
    ) -> Expiry {
        let mut state = self.lock();
        // The component's task ended because the item is being stopped or
        // preempted (or the engine is shutting down), not because the URL is
        // bad: leave without charging an attempt or calling the resolver.
        let still_this_run = !cancel.is_cancelled()
            && state.runners().get(&item_id).is_some_and(|runner| {
                runner.run_id == run_id && runner.retire_intent == RetireIntent::None
            });
        if !still_this_run {
            return Expiry::Fail(ComponentFailure::Cancelled);
        }

        let unrefreshable = || {
            Expiry::Fail(ComponentFailure::Download(DownloadError::ServerError {
                status: 403,
            }))
        };
        let Some(resolver) = self.resolver.clone() else {
            return unrefreshable();
        };
        // Swift's `itemComponent` and `itemSourceURL`.
        let Some((_, item)) = state.find_item(item_id) else {
            return unrefreshable();
        };
        let Some(component) = item.components.get(component_index).cloned() else {
            return unrefreshable();
        };
        if !matches!(component.origin, ComponentOrigin::Resolved { .. }) {
            return unrefreshable();
        }
        let source_url = item.source_url.clone();

        // A refresh consumes one attempt against the existing cap.
        let attempt = state.failed_attempts().get(&item_id).copied().unwrap_or(0) + 1;
        state.failed_attempts_mut().insert(item_id, attempt);
        if attempt >= self.retry_policy.max_attempts {
            state.failed_attempts_mut().remove(&item_id);
            return Expiry::Fail(refresh_failed(format!(
                "Gave up refreshing the download URL after {attempt} attempts"
            )));
        }
        Expiry::Refresh {
            resolver,
            source_url,
            component: Box::new(component),
        }
    }
}

fn refresh_failed(reason: String) -> ComponentFailure {
    ComponentFailure::Download(DownloadError::RefreshFailed { reason })
}

/// Swift interpolates the error value (`"\(error)"`), which prints the case
/// as written: `formatGone`, `ytDlpFailed(stderrTail: "...")`.
fn describe_resolve_error(error: &ResolveError) -> String {
    match error {
        ResolveError::BinaryMissing => "binaryMissing".to_string(),
        ResolveError::Unsupported => "unsupported".to_string(),
        ResolveError::FormatGone => "formatGone".to_string(),
        ResolveError::AuthRequired => "authRequired".to_string(),
        ResolveError::PrivateVideo => "privateVideo".to_string(),
        ResolveError::Unavailable => "unavailable".to_string(),
        ResolveError::DrmProtected => "drmProtected".to_string(),
        ResolveError::Timeout => "timeout".to_string(),
        ResolveError::YtDlpFailed { stderr_tail } => {
            format!("ytDlpFailed(stderrTail: {stderr_tail:?})")
        }
        ResolveError::Cancelled => "CancellationError()".to_string(),
    }
}
