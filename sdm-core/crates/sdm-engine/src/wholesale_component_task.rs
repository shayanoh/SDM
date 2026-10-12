//! Drives a `WholesaleDownloader` for one HLS/DASH-only component. Port of
//! `WholesaleComponentTask.swift`.
//!
//! # Cancellation
//!
//! Swift runs the downloader in an unstructured `Task` so `pause()` can
//! `cancel()` it. Here the downloader runs inside `start()` and `pause()`
//! cancels a `CancellationToken` it was handed. Unlike Swift's synchronous
//! `pause()`, [`WholesaleComponentTask::pause`] is `async` and returns only
//! once the downloader call has really stopped (the child process is gone),
//! so a caller can start the next attempt without the old `yt-dlp` still
//! writing to the same scratch files. Nothing is ever `abort`ed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sdm_core::{ByteRange, RangeSet, SourceUrl, WholesaleDownloader, WholesaleError};
use sdm_core::{WholesalePhase, WholesaleProgress};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::download_task::ComponentTelemetry;
use crate::yt_dlp_artifacts::YtDlpArtifacts;

/// Drives a `WholesaleDownloader` (yt-dlp as a downloader) for one
/// HLS/DASH-only component, exposing the slice of `DownloadTask`'s
/// interface the engine's `Runner` touches. Progress is a *synthesized*
/// contiguous `RangeSet` derived from the downloader's reports.
///
/// Resume: a pause / preempt keeps the yt-dlp scratch (`.part` / `.ytdl`
/// / fragments) on disk so the next run's `--continue` picks up from the
/// last completed fragment. A hard (non-cancellation) failure sweeps the
/// scratch so the next attempt starts clean. The component only reports
/// itself resumable once it has seen a *fragmented* progress report —
/// confirmation that yt-dlp's native (resumable) downloader is in use.
/// Parent specs `2026-09-03-multi-site-resolver-design.md` §6.7 and
/// `2026-09-03-wholesale-resume-design.md`.
pub struct WholesaleComponentTask {
    inner: Arc<Inner>,
}

struct Inner {
    item_id: Uuid,
    page_url: SourceUrl,
    format_selector: String,
    destination_path: PathBuf,
    downloader: Arc<dyn WholesaleDownloader>,
    /// Written by the downloader's `on_progress` (a sync callback off an
    /// arbitrary thread), read by the engine via the getters below.
    progress: Mutex<ProgressState>,
    /// Whether a downloader call is in flight. `pause()` cancels `cancel`
    /// under this lock, so `start()` cannot slip in between its check and
    /// its marking itself running.
    running: Mutex<bool>,
    /// Swift's `isCancelled` / `job.cancel()`: set by `pause()`, never
    /// cleared; handed to the downloader.
    cancel: CancellationToken,
    /// Signalled whenever `running` drops to `false`.
    stopped: Notify,
}

impl WholesaleComponentTask {
    pub fn new(
        item_id: Uuid,
        page_url: SourceUrl,
        format_selector: impl Into<String>,
        destination_path: PathBuf,
        downloader: Arc<dyn WholesaleDownloader>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                item_id,
                page_url,
                format_selector: format_selector.into(),
                destination_path,
                downloader,
                progress: Mutex::new(ProgressState::default()),
                running: Mutex::new(false),
                cancel: CancellationToken::new(),
                stopped: Notify::new(),
            }),
        }
    }

    // MARK: - DownloadTask-shaped interface

    pub fn id(&self) -> Uuid {
        self.inner.item_id
    }

    pub fn destination_path(&self) -> &Path {
        &self.inner.destination_path
    }

    /// Runs the downloader to completion and returns the finished output
    /// path. A pause surfaces as `WholesaleError::Cancelled` (Swift throws
    /// `WholesaleError.cancelled` from the same place).
    pub async fn start(&self) -> Result<PathBuf, WholesaleError> {
        let inner = &self.inner;
        {
            let mut running = inner.running.lock().unwrap();
            if inner.cancel.is_cancelled() {
                return Err(WholesaleError::Cancelled);
            }
            *running = true;
        }
        let result = {
            let _running = RunningGuard { inner };
            let on_progress = {
                let inner = Arc::clone(inner);
                Arc::new(move |progress: WholesaleProgress| inner.lock_progress().apply(progress))
            };
            inner
                .downloader
                .download(
                    &inner.page_url,
                    &inner.format_selector,
                    &inner.destination_path,
                    on_progress,
                    &inner.cancel,
                )
                .await
        };
        if let Err(error) = result {
            // Cancellation is a pause / preempt: keep every scratch file and
            // the synthesized progress so the next run resumes via
            // `--continue`. A real failure sweeps the scratch so the retry
            // starts clean.
            if error == WholesaleError::Cancelled {
                return Err(WholesaleError::Cancelled);
            }
            inner.cleanup_partial_output();
            inner.lock_progress().reset();
            return Err(error);
        }
        if !inner.destination_path.exists() {
            inner.cleanup_partial_output();
            inner.lock_progress().reset();
            return Err(WholesaleError::Failed {
                stderr_tail: "yt-dlp produced no output file".to_string(),
            });
        }
        // Success — remove any fragment / metadata siblings yt-dlp left, but
        // keep the finished output file itself.
        inner.cleanup_scratch_only();
        Ok(inner.destination_path.clone())
    }

    /// Cancels the downloader and returns once its call has stopped.
    ///
    /// Deliberately keeps the yt-dlp scratch and the synthesized progress:
    /// the next run resumes from the last completed fragment.
    pub async fn pause(&self) {
        let inner = &self.inner;
        {
            let _running = inner.running.lock().unwrap();
            inner.cancel.cancel();
        }
        loop {
            let notified = inner.stopped.notified();
            tokio::pin!(notified);
            // Registered before `running` is read, so a stop in between
            // cannot be missed.
            notified.as_mut().enable();
            if !*inner.running.lock().unwrap() {
                return;
            }
            notified.await;
        }
    }

    /// Synthesized `[0, downloaded_bytes)` — the one place a `RangeSet` is
    /// derived from an external reporter rather than from bytes SDM wrote.
    pub fn completed_ranges(&self) -> RangeSet {
        self.inner.lock_progress().completed_ranges()
    }

    pub fn expected_total_bytes(&self) -> Option<i64> {
        self.inner.lock_progress().total
    }

    pub fn active_worker_count(&self) -> usize {
        usize::from(self.inner.lock_progress().active)
    }

    pub fn peak_worker_count(&self) -> usize {
        1
    }

    /// `Some(false)` until a fragmented progress report confirms yt-dlp's
    /// native (resumable) downloader is in use — so the scheduler reserves
    /// this item's slot and never preempts it until resume is known to work.
    /// `Some(true)` afterward: the item is preemptible like any resumable
    /// download, and a pause / preempt keeps its fragments for `--continue`.
    pub fn probed_supports_ranges(&self) -> Option<bool> {
        Some(self.inner.lock_progress().saw_fragmented)
    }

    pub fn last_checkpoint_failure(&self) -> Option<String> {
        None
    }

    pub fn is_assembling(&self) -> bool {
        self.inner.lock_progress().phase == WholesalePhase::PostProcessing
    }

    /// See [`crate::DownloadTask::telemetry`].
    pub fn telemetry(&self) -> ComponentTelemetry {
        let progress = self.inner.lock_progress();
        ComponentTelemetry {
            completed_ranges: progress.completed_ranges(),
            expected_total_bytes: progress.total,
            active_worker_count: usize::from(progress.active),
            last_checkpoint_failure: None,
            is_assembling: progress.phase == WholesalePhase::PostProcessing,
        }
    }

    pub fn checkpoint_tick(&self) {}

    pub fn set_worker_count(&self, _count: usize) {}
}

impl Inner {
    fn lock_progress(&self) -> MutexGuard<'_, ProgressState> {
        self.progress.lock().unwrap()
    }

    // MARK: - Cleanup

    fn cleanup_partial_output(&self) {
        let Some((folder, filename)) = self.folder_and_filename() else {
            return;
        };
        // `destination_path` for a wholesale component is the item's final
        // output path, so sweep the output plus every yt-dlp scratch sibling
        // (`.ytdl`, `.part*`, `.fragN`, `Clip.fNNN.*`, `Clip.temp.*`).
        for path in YtDlpArtifacts::all_files(folder, filename) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Scratch siblings only — the finished output file stays.
    fn cleanup_scratch_only(&self) {
        let Some((folder, filename)) = self.folder_and_filename() else {
            return;
        };
        for path in YtDlpArtifacts::scratch_files(folder, filename) {
            let _ = std::fs::remove_file(path);
        }
    }

    fn folder_and_filename(&self) -> Option<(&Path, &str)> {
        let folder = self.destination_path.parent()?;
        let filename = self.destination_path.file_name()?.to_str()?;
        Some((folder, filename))
    }
}

/// Marks the downloader call finished — also when the `start()` future is
/// dropped mid-call — and wakes a waiting `pause()`.
struct RunningGuard<'a> {
    inner: &'a Inner,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        *self.inner.running.lock().unwrap() = false;
        self.inner.stopped.notify_waiters();
    }
}

/// The latest wholesale progress snapshot (Swift's `ProgressState`, whose
/// `NSLock` is the mutex around this).
struct ProgressState {
    downloaded: i64,
    total: Option<i64>,
    phase: WholesalePhase,
    active: bool,
    saw_fragmented: bool,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            downloaded: 0,
            total: None,
            phase: WholesalePhase::Downloading,
            active: false,
            saw_fragmented: false,
        }
    }
}

impl ProgressState {
    fn apply(&mut self, progress: WholesaleProgress) {
        self.active = true;
        self.phase = progress.phase;
        if progress.is_fragmented {
            self.saw_fragmented = true;
        }

        if let Some(downloaded) = progress.downloaded_bytes {
            self.downloaded = self.downloaded.max(downloaded);
        }

        // Total, re-derived on every report so the UI tracks yt-dlp's
        // moving estimate. For a fragmented (HLS/DASH) download there is
        // no real `total_bytes`; the reliable signal is the fragment
        // ratio, so `downloaded / fraction` is used — this makes the
        // synthesized progress bar exactly track yt-dlp's monotonic
        // fragment progress. `progress.total_bytes` (a real size, or
        // yt-dlp's float estimate before enough fragments exist) is the
        // fallback. Parent spec §6.7.
        match (progress.fraction, progress.total_bytes) {
            (Some(fraction), _) if fraction >= 0.005 && self.downloaded > 0 => {
                self.total = Some((self.downloaded as f64 / fraction) as i64);
            }
            (_, Some(reported)) => self.total = Some(reported),
            _ => {}
        }

        // A byte-less report that still carries a fraction (rare).
        if progress.downloaded_bytes.is_none()
            && let (Some(fraction), Some(total)) = (progress.fraction, self.total)
        {
            self.downloaded = self.downloaded.max((fraction * total as f64) as i64);
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn downloaded_bytes_clamped(&self) -> i64 {
        match self.total {
            Some(total) => self.downloaded.min(total),
            None => self.downloaded,
        }
    }

    fn completed_ranges(&self) -> RangeSet {
        let mut set = RangeSet::new();
        let done = self.downloaded_bytes_clamped();
        if done > 0 {
            set.insert(ByteRange::new(0, done));
        }
        set
    }
}
