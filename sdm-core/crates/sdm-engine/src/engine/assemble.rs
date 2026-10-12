//! Assembly: the step after every component has downloaded, and the
//! "Retry mux" action. Parent spec §7.2 / §9.3.

use sdm_core::{Assembly, DownloadItem, ItemState, MediaContainer};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{DownloadEngine, Inner};
use crate::muxer::MuxError;
use crate::resume_sidecar::ResumeSidecar;

/// How the assembly step ended.
pub(super) enum Assembled {
    /// The item lands in this state.
    Landed(ItemState),
    /// The step was cut short by the job being cancelled — the item was
    /// paused, preempted, or the engine is shutting down — rather than
    /// failing, so it must not be recorded as a failure.
    Interrupted,
}

impl Inner {
    /// Assembly step after every component completed. `Assembly::None` → each
    /// task already finalized its own file, nothing to do. `Mux` → run `ffmpeg
    /// -c copy` over the finalized parts into the output container, then
    /// delete the parts. Parent spec §7.2.
    pub(super) async fn assemble(&self, item_id: Uuid, cancel: &CancellationToken) -> Assembled {
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
    /// Shared by `assemble()` and the [`DownloadEngine::retry_mux`].
    ///
    /// A mux that ends because `cancel` fired — the item was paused,
    /// preempted or the engine is shutting down — is `Interrupted`, not a
    /// failure. (Swift's `catch` records any mux error, cancellation
    /// included, as `.failed("mux failed: CancellationError()")`, which
    /// overrides a user's Stop with a failure; the brief for this port rules
    /// that out.)
    pub(super) async fn run_mux(
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
}

impl DownloadEngine {
    /// Re-runs only the mux step for a `Failed` item whose components are all
    /// downloaded — the "Retry mux" action. Parent spec §7.2 / §9.3.
    ///
    /// A no-op unless the item is a `Mux` item in state `Failed` with every
    /// component complete. Like Swift's, it neither marks the item `Running`
    /// nor takes a runner: `is_assembling` shows while `ffmpeg` runs and the
    /// item lands `Completed` or `Failed` when it ends. The mux has no job to
    /// cancel here, so it runs under a token nothing fires.
    pub async fn retry_mux(&self, item_id: Uuid) {
        let inner = &self.inner;
        let (item, package_name) = {
            let state = inner.lock();
            let Some((package, item)) = state.find_item(item_id) else {
                return;
            };
            if item.assembly != Assembly::Mux
                || !matches!(item.state, ItemState::Failed { .. })
                || !item
                    .components
                    .iter()
                    .all(|component| component.is_complete())
            {
                return;
            }
            (item.clone(), package.name.clone())
        };
        let landing = match inner
            .run_mux(&item, &package_name, &CancellationToken::new())
            .await
        {
            Assembled::Landed(landing) => landing,
            // Unreachable: the token above is never cancelled.
            Assembled::Interrupted => return,
        };
        inner
            .lock()
            .mutate_item(item_id, |item| item.state = landing);
        inner.persist().await;
    }
}
