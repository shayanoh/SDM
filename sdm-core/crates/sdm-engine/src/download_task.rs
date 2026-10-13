//! Downloads one file using a pool of ephemeral workers that claim gaps from
//! a shared `RangeSet`. See spec §5. Port of `DownloadTask.swift`.
//!
//! # The suspension-point map
//!
//! The Swift `DownloadTask` is an actor: every stretch of code between two
//! `await`s runs without interleaving. The Rust port keeps that by holding
//! `Inner::state` (a `std::sync::Mutex`, never held across an `.await`) for
//! exactly those stretches. This is every `await` in the Swift source and
//! what may change across it — the map of the lock scopes below.
//!
//! * `start()`: `await prepare()`. Anything: `pause()` (sets `isPaused`; sees
//!   `file == nil`, so does nothing else), `setWorkerCount`, `checkpointTick`
//!   (a no-op, `file == nil`). Nothing else touches state yet.
//! * `start()`: `await runWorkers()`. Everything below, plus `pause()`
//!   closing the file mid-run.
//! * `prepare()`: `await transport.fetch` (the probe), and the second probe
//!   of `sourceURL` when the pinned mirror failed. `pause()` and
//!   `setWorkerCount` may land; `completed`, `file` and the totals are only
//!   written after the last probe, in one suspension-free stretch.
//! * `runWorkers()`: `await group.next()`. Workers finished, the target
//!   changed (`setWorkerCount`, `pause()` sets it to 0), `completed` and
//!   `reserved` moved, the file may have been closed. The top-up loop that
//!   follows re-reads all of it in one stretch.
//! * `awaitWorkerCountRaise()` (the sentinel): `for await _ in stream`.
//!   `signalWake` / `closeWakeChannel` are the only writers it cares about.
//! * `workerLoop(index:)`: `await download(...)`. Another worker may have
//!   stolen the tail of this worker's claim (`reserved[workerID]` shrunk),
//!   the target may have dropped below this worker's rank, `pause()` may have
//!   closed the file, and `completed` has grown.
//! * `download(_:workerID:index:)`: `await transport.fetch`. As above.
//! * `download(...)`: `for try await chunk in response.body`, once per chunk.
//!   As above. This is the one that matters most: after it resumes, the live
//!   claim end is re-read (`reserved[workerID]?.end`) and the file is
//!   re-checked, and the write plus `record` that follow are one
//!   suspension-free stretch, so a byte is recorded if and only if its write
//!   just succeeded on an open file. In Rust that stretch is one lock scope
//!   that includes the `pwrite` itself.
//!
//! `pause()`, `setWorkerCount`, `checkpointTick` and the getters are the
//! actor's entry points and each is one lock scope.
//!
//! # Cancellation
//!
//! Swift's implicit task cancellation is an explicit `CancellationToken`
//! here. `pause()` (and a failed worker, and the `start()` future being
//! dropped) cancel it; a worker that notices it stops exactly as it does
//! when the file has gone, so its recorded bytes are the bytes it wrote.
//! Nothing is ever `abort`ed.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use futures::StreamExt;
use sdm_core::{AppTiming, ByteRange, RangeSet, SourceUrl};
use thiserror::Error;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use crate::http_transport::{HttpTransport, RangeRequest, RangeResponse, TransportError};
use crate::resume_sidecar::ResumeSidecar;
use crate::sparse_file::{FinalizeError, SparseFile, SparseFileError};

/// Everything `DownloadTask::start` can fail with. Swift's `start()` throws
/// `any Error`; the transport, file and rename failures it passes through are
/// carried as variants so `RetryPolicy::classify` can still see them.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error("the server did not report a size")]
    UnknownSize,
    #[error("server error: HTTP {status}")]
    ServerError { status: u16 },
    /// A `403`/`410` on a component whose URL can be refreshed by a
    /// `LinkResolver` (a signed `googlevideo` URL that has expired). Carries
    /// the yt-dlp `format_id` the engine needs to request a fresh URL.
    #[error("the URL for format {format_id} expired")]
    UrlExpired { format_id: String },
    /// The resolver could not hand back a usable fresh URL for an expired
    /// component — the format is gone, its size changed, or the resolve
    /// itself failed. Terminal.
    #[error("refreshing the URL failed: {reason}")]
    RefreshFailed { reason: String },
    /// A worker whose claim starts past byte 0 asked for a `Range` and was
    /// answered with something other than `206`. The body of such an answer
    /// starts at byte 0, so writing it at the claim's offset would corrupt the
    /// file; the attempt fails before any byte is written. Transient (the
    /// retry policy decides how often to try again).
    #[error(
        "the server advertised byte ranges but answered a ranged request at offset {start} \
         with HTTP {status} instead of 206"
    )]
    RangeNotHonored { status: u16, start: i64 },
    #[error("workers finished but the download is incomplete")]
    IncompleteAfterWorkersFinished,
    /// The origin's response body ended before the claimed range was fully
    /// delivered, without the transport throwing — a clean short read. Bytes
    /// already written for the claim remain on disk and recorded in
    /// `completed_ranges`; only the unwritten remainder is lost.
    #[error("truncated response: expected {expected} bytes, received {received}")]
    TruncatedResponse { expected: i64, received: i64 },
    /// A fetch failed (Swift lets the transport's error propagate as is).
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// Opening, writing or flushing the partial file failed.
    #[error(transparent)]
    File(#[from] SparseFileError),
    /// Renaming the finished file into place failed — almost always because
    /// something already occupies the destination (`kind` is then
    /// `AlreadyExists`). Swift passes the Cocoa error through; this keeps the
    /// `io::Error`'s kind and text, which is all `classify` reads.
    #[error("could not move the finished file into place: {message}")]
    Move {
        kind: io::ErrorKind,
        message: String,
    },
    /// The source URL has no parsed form the transport could fetch (only a
    /// deserialized string the `url` crate rejects). Swift's `URL` cannot be
    /// in this state.
    #[error("{url} is not a URL that can be fetched")]
    InvalidUrl { url: String },
}

impl From<FinalizeError> for DownloadError {
    fn from(error: FinalizeError) -> Self {
        match error {
            FinalizeError::Sync(error) => Self::File(error),
            FinalizeError::Move(error) => Self::Move {
                kind: error.kind(),
                message: error.to_string(),
            },
        }
    }
}

/// One component's live figures as `DownloadEngine::snapshot()` needs them.
/// A single value so a snapshot costs one lock per component.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentTelemetry {
    pub completed_ranges: RangeSet,
    pub expected_total_bytes: Option<i64>,
    pub active_worker_count: usize,
    pub last_checkpoint_failure: Option<String>,
    pub is_assembling: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadTaskConfiguration {
    pub worker_count: usize,
    pub min_chunk: i64,
    /// Bytes written per worker between sidecar checkpoints.
    pub checkpoint_interval: i64,
    /// Elapsed heartbeat ticks without a checkpoint before
    /// `checkpoint_tick()` forces one — the wall-clock half of spec
    /// §4.3's "every ~8 MB per worker or every 5 s, whichever comes
    /// first". Five seconds' worth of ticks at `AppTiming::TICKS_PER_SECOND`.
    pub checkpoint_staleness_ticks: usize,
    /// Receive previously stored completed so UI can display its status.
    /// It will be automatically updated from sidecar when fetch starts.
    pub cached_completed: Option<RangeSet>,
    /// When set, a `403`/`410` becomes `DownloadError::UrlExpired` (a
    /// signed URL the engine can refresh) rather than a plain
    /// `ServerError`. It carries the yt-dlp `format_id`.
    pub refreshable_format_id: Option<String>,
    /// Heartbeat ticks between `fsync`s of the partial file. The
    /// sidecar is rewritten on every checkpoint, but flushing the file
    /// to the device on each one cost about as much time as writing the
    /// data did; once a minute bounds what a power cut can lose to a
    /// minute's worth, and a crash or force quit still loses nothing.
    pub sync_interval_ticks: usize,
    /// Identifies the current boot — see
    /// [`ResumeSidecar::resumable_ranges`]. Injected so a test can stand in
    /// for a restart.
    pub boot_session_id: Option<String>,
}

impl DownloadTaskConfiguration {
    /// A configuration with Swift's defaults for the optional parameters:
    /// five seconds of staleness, a flush a minute, this boot's session id.
    ///
    /// # Panics
    ///
    /// Panics unless `worker_count >= 1`, `min_chunk > 0` and
    /// `checkpoint_interval > 0` (Swift's `precondition`s).
    pub fn new(
        worker_count: usize,
        min_chunk: i64,
        checkpoint_interval: i64,
        cached_completed: Option<RangeSet>,
    ) -> Self {
        let configuration = Self {
            worker_count,
            min_chunk,
            checkpoint_interval,
            checkpoint_staleness_ticks: AppTiming::TICKS_PER_SECOND * 5,
            cached_completed,
            refreshable_format_id: None,
            sync_interval_ticks: AppTiming::TICKS_PER_SECOND * 60,
            boot_session_id: ResumeSidecar::current_boot_session_id(),
        };
        configuration.validate();
        configuration
    }

    pub fn with_checkpoint_staleness_ticks(mut self, ticks: usize) -> Self {
        self.checkpoint_staleness_ticks = ticks;
        self.validate();
        self
    }

    pub fn with_refreshable_format_id(mut self, format_id: impl Into<String>) -> Self {
        self.refreshable_format_id = Some(format_id.into());
        self
    }

    pub fn with_sync_interval_ticks(mut self, ticks: usize) -> Self {
        self.sync_interval_ticks = ticks;
        self.validate();
        self
    }

    pub fn with_boot_session_id(mut self, boot_session_id: Option<String>) -> Self {
        self.boot_session_id = boot_session_id;
        self
    }

    /// The fields are public, so `DownloadTask::new` checks them again.
    fn validate(&self) {
        assert!(
            self.sync_interval_ticks > 0,
            "syncIntervalTicks must be positive"
        );
        assert!(self.worker_count >= 1, "workerCount must be at least 1");
        assert!(self.min_chunk > 0, "minChunk must be positive");
        assert!(
            self.checkpoint_interval > 0,
            "checkpointInterval must be positive"
        );
        assert!(
            self.checkpoint_staleness_ticks > 0,
            "checkpointStalenessTicks must be positive"
        );
    }
}

/// What a finished child task was, so the drain loop can tell a worker
/// that ran out of work from the sentinel that woke it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupOutcome {
    Worker,
    Sentinel,
}

type WorkerSet = JoinSet<Result<GroupOutcome, DownloadError>>;

struct State {
    configuration: DownloadTaskConfiguration,
    /// The URL workers actually fetch from, pinned to whatever `source_url`
    /// resolved to (after redirects) on the probe that validated this run's
    /// resume — see `resolved_url` on `ResumeSidecar`. `source_url`'s own
    /// parsed form until `prepare()` runs.
    effective_url: Option<Url>,
    completed: RangeSet,
    /// Keyed by worker index (never reused), which does the job of Swift's
    /// per-worker `UUID` and gives the stealing scan a stable order.
    reserved: BTreeMap<usize, ByteRange>,
    /// Absolute byte offset each busy worker has written up to within its
    /// own `reserved` claim. Lets `claim_next` compute, for any in-flight
    /// worker, exactly how much of its claim is still unwritten — the
    /// `RangeSet` alone cannot answer that, since `completed` only ever
    /// records finished ranges, not a worker's live progress through a
    /// range it has not finished yet.
    write_cursor: BTreeMap<usize, i64>,
    total_bytes: i64,
    validator: Option<String>,
    file: Option<SparseFile>,
    bytes_since_checkpoint: i64,
    /// Elapsed ticks since the last checkpoint, for the wall-clock trigger
    /// driven by `checkpoint_tick()`.
    ticks_since_checkpoint: usize,
    /// The part of `completed` known to have reached the storage device:
    /// its value at the last `fsync`. What a sidecar records as
    /// `durable_completed`.
    durable: RangeSet,
    /// Elapsed heartbeat ticks since the last `fsync`, advanced by
    /// `checkpoint_tick()`.
    ticks_since_sync: usize,
    accepts_ranges: bool,
    /// Why the most recent checkpoint failed, or `None` if the last one
    /// succeeded. A sidecar that cannot be written means a crash loses all
    /// progress for this download, which the user has to be able to see.
    last_checkpoint_failure: Option<String>,
    /// How many checkpoints have failed during this run. A single failure can
    /// be a transient hiccup; a rising count is a broken destination.
    checkpoint_failure_count: usize,
    /// Set by `pause()` and never cleared. `start()` awaits `transport.fetch`
    /// inside `prepare()`; a `pause()` landing on that suspension point used
    /// to be lost, because it set `target_worker_count = 0` (or, with `file`
    /// still `None`, returned before even doing that) and `run_workers()` then
    /// reset the target to the configured count and downloaded the whole file
    /// anyway. The engine preempts items by pausing them, so it reaches that
    /// window for real. Recording the request in a flag `run_workers()`
    /// consults closes it. Terminal by design: a paused task is finished, and
    /// the engine builds a fresh `DownloadTask` for the next attempt.
    is_paused: bool,
    /// The number of workers the pool is currently trying to keep running.
    target_worker_count: usize,
    /// Indices of workers currently inside `worker_loop`.
    live_worker_indices: BTreeSet<usize>,
    /// Monotonic source of worker indices; never reused.
    next_worker_index: usize,
    /// High-water mark of `live_worker_indices.len()` for the current run.
    peak_live_worker_count: usize,
    /// A raise that arrived while no sentinel was parked.
    pending_wake: bool,
    /// Set once the download is finished; makes the sentinel return rather
    /// than park, so the worker group can drain to empty.
    wake_closed: bool,
    /// Every byte range actually written to disk, in the order `record()`
    /// saw them. Test-only (Swift's `#if DEBUG`): once claims can be split
    /// mid-flight, `completed_ranges` alone can't prove two workers never
    /// wrote the same byte, because `RangeSet::insert` silently merges
    /// overlapping ranges rather than rejecting them. This is the direct
    /// witness.
    #[cfg(debug_assertions)]
    write_log: Vec<ByteRange>,
}

struct Inner {
    id: Uuid,
    source_url: SourceUrl,
    destination_path: PathBuf,
    sidecar_path: PathBuf,
    transport: Arc<dyn HttpTransport>,
    state: Mutex<State>,
    /// Cancelled by `pause()`, by a failed worker and when `start()` is
    /// dropped; every worker checks it between chunks and while fetching.
    cancel: CancellationToken,
    /// Wakes the sentinel parked in `await_worker_count_raise`, standing in
    /// for Swift's `AsyncStream` wake channel.
    wake: Notify,
}

/// Downloads one file using a pool of ephemeral workers that claim gaps from
/// a shared `RangeSet`. See spec §5.
pub struct DownloadTask {
    inner: Arc<Inner>,
}

impl DownloadTask {
    /// # Panics
    ///
    /// Panics if `configuration` breaks one of its preconditions (see
    /// [`DownloadTaskConfiguration::new`]).
    pub fn new(
        id: Uuid,
        source_url: SourceUrl,
        destination_path: PathBuf,
        transport: Arc<dyn HttpTransport>,
        configuration: DownloadTaskConfiguration,
    ) -> Self {
        configuration.validate();
        let sidecar_path = ResumeSidecar::path_for(&destination_path);
        let state = State {
            effective_url: source_url.as_url().cloned(),
            completed: configuration.cached_completed.clone().unwrap_or_default(),
            configuration,
            reserved: BTreeMap::new(),
            write_cursor: BTreeMap::new(),
            total_bytes: 0,
            validator: None,
            file: None,
            bytes_since_checkpoint: 0,
            ticks_since_checkpoint: 0,
            durable: RangeSet::new(),
            ticks_since_sync: 0,
            accepts_ranges: true,
            last_checkpoint_failure: None,
            checkpoint_failure_count: 0,
            is_paused: false,
            target_worker_count: 1,
            live_worker_indices: BTreeSet::new(),
            next_worker_index: 0,
            peak_live_worker_count: 0,
            pending_wake: false,
            wake_closed: false,
            #[cfg(debug_assertions)]
            write_log: Vec::new(),
        };
        Self {
            inner: Arc::new(Inner {
                id,
                source_url,
                destination_path,
                sidecar_path,
                transport,
                state: Mutex::new(state),
                cancel: CancellationToken::new(),
                wake: Notify::new(),
            }),
        }
    }

    pub fn id(&self) -> Uuid {
        self.inner.id
    }

    pub fn source_url(&self) -> &SourceUrl {
        &self.inner.source_url
    }

    pub fn destination_path(&self) -> &Path {
        &self.inner.destination_path
    }

    pub fn completed_ranges(&self) -> RangeSet {
        self.inner.lock().completed.clone()
    }

    pub fn active_worker_count(&self) -> usize {
        self.inner.lock().reserved.len()
    }

    /// Everything `DownloadEngine::snapshot()` reads from a task, in one
    /// lock instead of one per figure.
    pub fn telemetry(&self) -> ComponentTelemetry {
        let state = self.inner.lock();
        ComponentTelemetry {
            completed_ranges: state.completed.clone(),
            expected_total_bytes: Inner::expected_total_bytes(&state),
            active_worker_count: state.reserved.len(),
            last_checkpoint_failure: state.last_checkpoint_failure.clone(),
            is_assembling: false,
        }
    }

    /// The most workers that were ever live at once during this run. Lets a
    /// caller (and the tests) observe the concurrency a `set_worker_count`
    /// raise actually achieved, which byte identity alone cannot show.
    pub fn peak_worker_count(&self) -> usize {
        self.inner.lock().peak_live_worker_count
    }

    /// Whether the origin honored `Range` requests on the probe. `false`
    /// forces the pool to a single worker (spec §5.3).
    pub fn supports_ranges(&self) -> bool {
        self.inner.lock().accepts_ranges
    }

    /// The resource's total size as reported by the probe, or `None` before
    /// `prepare()` has run. The engine folds this into its snapshots so the
    /// UI can show a fraction rather than a bare byte count.
    pub fn expected_total_bytes(&self) -> Option<i64> {
        Inner::expected_total_bytes(&self.inner.lock())
    }

    /// `supports_ranges`, but `None` until the probe has actually answered.
    ///
    /// `accepts_ranges` starts optimistically `true`, so reading it before
    /// `prepare()` returns would report a resumability the origin has never
    /// confirmed. `total_bytes` is assigned in the same lock scope that
    /// assigns `accepts_ranges`, so a non-zero size is an exact witness that
    /// the probe landed. The engine mirrors this onto
    /// `DownloadItem::is_resumable`.
    pub fn probed_supports_ranges(&self) -> Option<bool> {
        let state = self.inner.lock();
        (state.total_bytes > 0).then_some(state.accepts_ranges)
    }

    /// Why the most recent checkpoint failed, or `None` if it succeeded.
    pub fn last_checkpoint_failure(&self) -> Option<String> {
        self.inner.lock().last_checkpoint_failure.clone()
    }

    /// How many checkpoints have failed during this run.
    pub fn checkpoint_failure_count(&self) -> usize {
        self.inner.lock().checkpoint_failure_count
    }

    /// Every range written to disk, in the order it was recorded. Test-only,
    /// like Swift's `writeLog`: present in debug builds.
    #[cfg(debug_assertions)]
    pub fn write_log(&self) -> Vec<ByteRange> {
        self.inner.lock().write_log.clone()
    }

    /// Probes the origin, then runs the worker pool until the file is complete.
    ///
    /// Dropping the returned future abandons the download the way a
    /// cancelled Swift task does: it behaves as [`DownloadTask::pause`] (one
    /// last flushed checkpoint, then the file is closed) and the workers stop.
    pub async fn start(&self) -> Result<PathBuf, DownloadError> {
        let guard = AbandonGuard {
            inner: &self.inner,
            armed: true,
        };
        let result = self.inner.start().await;
        guard.disarm();
        result
    }

    /// Stops workers and flushes resume state to disk.
    ///
    /// The partial file and its sidecar are left in place; a later `start()`
    /// on a fresh task resumes from them. Closing the descriptor here can
    /// race a worker mid-`download`; the worker's write and its recording of
    /// the bytes are one lock scope that re-checks the file, so once `pause`
    /// holds the lock no byte can be written or recorded afterwards, and the
    /// sidecar written here describes exactly the bytes on disk. The workers
    /// are also cancelled, so they drop their requests and stop at once.
    ///
    /// A no-op when nothing is in flight (`file == None`): once `start()` has
    /// already finished — successfully or not — there is nothing to stop,
    /// and calling `checkpoint()` unconditionally here would resurrect a
    /// sidecar describing a finished (or never-started) download. For a
    /// successful completion in particular, `file` is `None` and `completed` /
    /// `total_bytes` / `validator` still hold their *finished* values, so an
    /// unconditional checkpoint would write a `.sdmpart` claiming the whole
    /// file is a complete in-progress download, even though the
    /// `.incomplete` file backing it no longer exists (it was already
    /// renamed to the final destination). A later `start()` against the same
    /// destination would then trust that sidecar, open a fresh all-zero
    /// `.incomplete` file, believe it complete without downloading anything,
    /// and either fail to finalize (destination already exists) or — if the
    /// destination was deleted in the meantime — silently move an all-zero
    /// file into place. This is a UI race that's reachable in production
    /// (user hits Pause just as the transfer completes), so it is guarded
    /// here rather than relying solely on `prepare()`'s backing-file check.
    pub fn pause(&self) {
        self.inner.pause();
    }

    /// Changes the number of concurrent workers mid-download.
    ///
    /// Raising it spawns more as slots free; lowering it retires surplus
    /// workers after they finish their current claim. Their partial progress is
    /// already in `completed`, so survivors simply pick up the resulting gaps.
    ///
    /// # Panics
    ///
    /// Panics if `count` is zero ("workerCount must be at least 1").
    pub fn set_worker_count(&self, count: usize) {
        assert!(count >= 1, "workerCount must be at least 1");
        let inner = &self.inner;
        let is_raise = {
            let mut state = inner.lock();
            let is_raise = count > state.target_worker_count;
            state.target_worker_count = count;
            state.configuration.worker_count = count;
            is_raise
        };
        // Lowering needs no nudge: retiring workers return on their own and
        // that already drives the drain loop. Raising has to wake it, since
        // busy workers never return between claims.
        if is_raise {
            inner.signal_wake();
        }
    }

    /// Drives the wall-clock half of spec §4.3's checkpoint trigger: "every
    /// ~8 MB per worker or every 5 s, whichever comes first." `record`
    /// already implements the byte half; this is meant to be called once per
    /// heartbeat tick by the engine, so a slow or drip-feeding server that
    /// never reaches `checkpoint_interval` bytes still gets a sidecar written
    /// periodically instead of losing unbounded wall-clock progress on a
    /// crash.
    ///
    /// A no-op once the download has finished or never started (`file ==
    /// None`) — same guard as `pause()` — so a stray tick can't resurrect a
    /// sidecar for a completed, failed-and-abandoned, or never-started
    /// download. `checkpoint()` itself resets this counter, whether it fired
    /// from here or from the byte trigger in `record`, so the two triggers
    /// can never both write for the same span of progress.
    pub fn checkpoint_tick(&self) {
        let inner = &self.inner;
        let mut state = inner.lock();
        if state.file.is_none() {
            return;
        }
        state.ticks_since_checkpoint += 1;
        state.ticks_since_sync += 1;
        if state.ticks_since_checkpoint < state.configuration.checkpoint_staleness_ticks {
            return;
        }
        inner.checkpoint(&mut state, false);
    }
}

/// Abandons the download when the `start()` future is dropped before it
/// finishes: the equivalent of the cancelled Swift task unwinding through
/// `start()`'s error path.
struct AbandonGuard<'a> {
    inner: &'a Arc<Inner>,
    armed: bool,
}

impl AbandonGuard<'_> {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for AbandonGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.inner.pause();
        }
    }
}

/// Owns the worker `JoinSet`. A `JoinSet` aborts its tasks when dropped, and
/// aborting is not how this port stops anything: dropping the group cancels
/// the token and detaches the tasks instead, so each one stops on its own at
/// its next check.
struct WorkerGroup {
    set: WorkerSet,
    inner: Arc<Inner>,
}

impl Drop for WorkerGroup {
    fn drop(&mut self) {
        self.inner.cancel.cancel();
        {
            let mut state = self.inner.lock();
            self.inner.close_wake(&mut state);
        }
        self.set.detach_all();
    }
}

/// Releases a worker's claim and index however its task ends — Swift's
/// `defer` in `workerLoop`.
struct WorkerCleanup {
    inner: Arc<Inner>,
    index: usize,
}

impl Drop for WorkerCleanup {
    fn drop(&mut self) {
        let mut state = self.inner.lock();
        state.reserved.remove(&self.index);
        state.write_cursor.remove(&self.index);
        state.live_worker_indices.remove(&self.index);
        crate::engine_log!(
            "{} closed",
            crate::worker_tag(self.inner.id, &self.inner.file_name(), self.index)
        );
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn file_name(&self) -> String {
        self.destination_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn expected_total_bytes(state: &State) -> Option<i64> {
        (state.total_bytes > 0).then_some(state.total_bytes)
    }

    /// A `403`/`410` is a signed-URL expiry the engine can refresh when this
    /// component came from a resolver; otherwise it is a plain server error.
    fn status_error(&self, status: u16) -> DownloadError {
        if (status == 403 || status == 410)
            && let Some(format_id) = &self.lock().configuration.refreshable_format_id
        {
            return DownloadError::UrlExpired {
                format_id: format_id.clone(),
            };
        }
        DownloadError::ServerError { status }
    }

    async fn start(self: &Arc<Self>) -> Result<PathBuf, DownloadError> {
        self.prepare().await?;

        let outcome = match self.run_workers().await {
            Ok(()) => {
                let state = self.lock();
                if state.completed.is_complete(state.total_bytes) {
                    Ok(())
                } else {
                    Err(DownloadError::IncompleteAfterWorkersFinished)
                }
            }
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            // Close the descriptor on any failure path so a caller that
            // retains a failed task (e.g. to inspect completed_ranges before
            // retrying) doesn't leak the fd until the task drops.
            // `.incomplete` and the sidecar are left in place — a failed
            // download must stay resumable. `SparseFile::close` is
            // idempotent, so this can't fight with `pause()`.
            //
            // One last flushed checkpoint first (a no-op once `pause()` has
            // already cleared `file`): the sidecar then vouches for every
            // byte this attempt wrote, rather than only what the last
            // once-a-minute flush covered. Skipped when nothing was written
            // at all, so an attempt that never got a byte leaves no sidecar.
            let mut state = self.lock();
            if !state.completed.ranges().is_empty() {
                self.checkpoint(&mut state, true);
            }
            if let Some(file) = &state.file {
                file.close();
            }
            return Err(error);
        }

        let mut state = self.lock();
        let Some(file) = state.file.as_ref() else {
            return Err(DownloadError::IncompleteAfterWorkersFinished);
        };
        match file.finalize() {
            Ok(result) => {
                state.file = None;
                ResumeSidecar::remove(&self.sidecar_path);
                Ok(result)
            }
            Err(error) => {
                // The rename failed — almost always because something already
                // occupies the destination. Refusing to overwrite is the
                // intended behavior, but the descriptor must still be
                // released and `file` cleared, or a later `pause()` would
                // checkpoint a sidecar claiming the whole file is complete
                // while the bytes have not been moved into place. The
                // `.incomplete` file and its existing sidecar stay, so
                // nothing already downloaded is lost.
                file.close();
                state.file = None;
                Err(error.into())
            }
        }
    }

    /// Probes the resource and opens the destination file.
    ///
    /// Resume only happens when the sidecar is present, readable, still
    /// describes the same remote resource (spec §5.3), *and* the
    /// `.incomplete` file it refers to actually exists and is large enough to
    /// back the byte ranges it claims. That last check is a second line of
    /// defense against a stale or poisoned sidecar (e.g. one written after
    /// the real `.incomplete` file was already renamed away by a successful
    /// `finalize()`) — trusting such a sidecar would mark the whole file
    /// "complete" over a freshly-created, all-zero `.incomplete` file and
    /// silently finalize garbage. Anything that fails any of these checks —
    /// no sidecar, a corrupt one, a validator mismatch, or a missing/short
    /// backing file — restarts at zero rather than risk stitching new bytes
    /// onto stale (or nonexistent) ones.
    async fn prepare(&self) -> Result<(), DownloadError> {
        let source =
            self.source_url
                .as_url()
                .cloned()
                .ok_or_else(|| DownloadError::InvalidUrl {
                    url: self.source_url.as_str().to_string(),
                })?;

        // Probing whichever URL the *previous* run actually landed on (when
        // known) rather than always re-resolving `source_url` keeps the
        // validator comparison below meaningful: a redirecting origin like
        // Fedora's MirrorManager can send a fresh probe to a different mirror
        // than the one that wrote the bytes on disk, which would otherwise
        // make a perfectly good resume look like the remote file changed.
        let existing_sidecar = ResumeSidecar::load(&self.sidecar_path);
        let pinned_url = existing_sidecar
            .as_ref()
            .and_then(|sidecar| sidecar.resolved_url.as_ref())
            .and_then(SourceUrl::as_url)
            .cloned();
        let mut probe = self
            .probe_fetch(RangeRequest::new(
                pinned_url.clone().unwrap_or_else(|| source.clone()),
                Some(ByteRange::new(0, 1)),
            ))
            .await?;
        // The pinned mirror from a prior run may have gone away entirely
        // between sessions; re-resolve `source_url` once rather than failing
        // a download that would otherwise succeed against any other mirror.
        // A fresh mirror here still goes through the normal validator
        // comparison below, so a genuine remote change is still caught.
        if pinned_url.is_some() && !(200..300).contains(&probe.status_code) {
            probe = self
                .probe_fetch(RangeRequest::new(source, Some(ByteRange::new(0, 1))))
                .await?;
        }
        if !(200..300).contains(&probe.status_code) {
            return Err(self.status_error(probe.status_code));
        }
        let Some(size) = probe.total_size.filter(|size| *size > 0) else {
            return Err(DownloadError::UnknownSize);
        };

        let mut state = self.lock();
        state.total_bytes = size;
        state.validator = probe.validator.clone();
        state.effective_url = Some(probe.resolved_url.clone());

        // A server that ignores Range cannot be segmented: every worker would
        // receive the whole body and overwrite the others' offsets. It also
        // cannot be resumed: every fetch against it returns the whole body
        // starting at byte 0, so a later run stitching new bytes onto an
        // old, non-zero `completed` set would misalign the write offset
        // against the response and corrupt the file — the same failure mode
        // `claim_next` guards against within a single run, but a sidecar
        // reintroduces it across runs. Per spec §5.3, no Range support means
        // no checkpointing, so `accepts_ranges` gates the resume decision
        // below and `checkpoint()` is a no-op for the rest of this run.
        state.accepts_ranges = probe.accepts_ranges;
        if !state.accepts_ranges {
            state.configuration.worker_count = 1;
            state.target_worker_count = 1;
        }

        let incomplete_path = SparseFile::incomplete_path(&self.destination_path);
        let resumable = if state.accepts_ranges
            && let Some(sidecar) = &existing_sidecar
            && sidecar.matches(size, probe.validator.as_deref())
            && let Some(on_disk_size) = Self::file_size(&incomplete_path)
            && on_disk_size >= size
        {
            Some(sidecar.resumable_ranges(state.configuration.boot_session_id.as_deref()))
        } else {
            None
        };
        match resumable {
            Some(ranges) => state.completed = ranges,
            None => {
                state.completed = RangeSet::new();
                ResumeSidecar::remove(&self.sidecar_path);
                let _ = std::fs::remove_file(&incomplete_path);
            }
        }

        let opened = SparseFile::open(&self.destination_path, size)?;
        // Resumed bytes may still be only in the page cache (the previous
        // run does not flush on every checkpoint). One flush here makes
        // them durable before any sidecar of this run vouches for them; if
        // it fails, this run simply vouches for nothing until its first
        // successful flush.
        state.durable = if state.completed.ranges().is_empty() || opened.sync().is_err() {
            RangeSet::new()
        } else {
            state.completed.clone()
        };
        state.file = Some(opened);
        state.ticks_since_sync = 0;
        Ok(())
    }

    /// One of `prepare`'s probe requests, abandoned the moment the task is
    /// cancelled (a stop, a removal or a quit), as URLSession abandons a
    /// request when its Swift task is cancelled. Without the race a stop waits
    /// out the transport's connect and header timeouts.
    ///
    /// A cancelled probe fails the attempt with
    /// [`DownloadError::IncompleteAfterWorkersFinished`], which is what
    /// `start()` reports when a pause lands during the probe and the probe
    /// then returns: transient, so the engine returns the item to `Queued`
    /// (or ignores the error when it is retiring the runner).
    async fn probe_fetch(&self, request: RangeRequest) -> Result<RangeResponse, DownloadError> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(DownloadError::IncompleteAfterWorkersFinished),
            fetched = self.transport.fetch(request) => Ok(fetched?),
        }
    }

    fn file_size(path: &Path) -> Option<i64> {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| metadata.len() as i64)
    }

    fn pause(&self) {
        {
            let mut state = self.lock();
            // Recorded before the `file` guard on purpose: a pause that
            // lands while `prepare()` is still awaiting `transport.fetch`
            // finds `file` `None` and would otherwise leave no trace at all,
            // letting `run_workers()` proceed as if nothing had happened.
            state.is_paused = true;
            if state.file.is_some() {
                state.target_worker_count = 0;
                // A deliberate stop flushes: what the sidecar records is then
                // durable in full, whatever happens to the machine afterwards.
                self.checkpoint(&mut state, true);
                if let Some(file) = &state.file {
                    file.close();
                }
                state.file = None;
            }
        }
        self.cancel.cancel();
    }

    async fn run_workers(self: &Arc<Self>) -> Result<(), DownloadError> {
        let mut group = WorkerGroup {
            set: JoinSet::new(),
            inner: self.clone(),
        };
        // One lock scope from the `is_paused` check through spawning the
        // initial workers and the sentinel, as the Swift actor runs them with
        // no suspension in between. Splitting it would let `pause()` (which
        // sets the target to 0) land after the target reset and before the
        // spawn: no worker would start and the sentinel would park forever.
        {
            let mut state = self.lock();
            // A pause that arrived while `prepare()` was suspended must not
            // be undone by the target reset below. Close the descriptor
            // `prepare()` just opened and spawn nothing; `start()` then finds
            // the file incomplete and fails, which the engine classifies as
            // transient and returns to `queued`. The `.incomplete` file and
            // any sidecar stay on disk, so the next attempt resumes.
            if state.is_paused {
                if let Some(file) = &state.file {
                    file.close();
                }
                state.file = None;
                return Ok(());
            }
            state.target_worker_count = state.configuration.worker_count;
            state.wake_closed = false;
            state.pending_wake = false;
            while state.live_worker_indices.len() < state.target_worker_count {
                self.spawn_worker(&mut state, &mut group.set);
            }
            self.add_sentinel(&mut group.set);
        }

        // Drain finished children. A child finishes either because it ran
        // out of claimable work / was retired, or because it is the
        // sentinel and `set_worker_count` raised the target. Both cases want
        // the same response: top the pool back up to the current target.
        while let Some(joined) = group.set.join_next().await {
            let outcome = match joined {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(error)) => {
                    // The first failure wins. The other children are told to
                    // stop and awaited, as a throwing task group does, so the
                    // caller's error path sees a quiet pool.
                    self.cancel.cancel();
                    {
                        let mut state = self.lock();
                        self.close_wake(&mut state);
                    }
                    while group.set.join_next().await.is_some() {}
                    return Err(error);
                }
                Err(join_error) => {
                    if join_error.is_panic() {
                        std::panic::resume_unwind(join_error.into_panic());
                    }
                    // Nothing aborts these tasks, so this is unreachable;
                    // fail the run rather than loop.
                    return Err(DownloadError::IncompleteAfterWorkersFinished);
                }
            };

            let mut state = self.lock();
            while state.live_worker_indices.len() < state.target_worker_count
                && state.has_work_remaining()
            {
                self.spawn_worker(&mut state, &mut group.set);
            }

            if state.live_worker_indices.is_empty() {
                // No worker is live and the top-up spawned none, so
                // `has_work_remaining()` was false with an empty `reserved`
                // — the file is done. Close the wake channel so the
                // sentinel returns and is not replaced; the group can then
                // drain to empty and `join_next` yields `None`.
                self.close_wake(&mut state);
            } else if outcome == GroupOutcome::Sentinel && !self.cancel.is_cancelled() {
                self.add_sentinel(&mut group.set);
            }
        }
        Ok(())
    }

    fn spawn_worker(self: &Arc<Self>, state: &mut State, set: &mut WorkerSet) {
        let index = state.next_worker_index;
        state.next_worker_index += 1;
        state.live_worker_indices.insert(index);
        state.peak_live_worker_count = state
            .peak_live_worker_count
            .max(state.live_worker_indices.len());
        let inner = self.clone();
        set.spawn(async move {
            inner.worker_loop(index).await?;
            Ok(GroupOutcome::Worker)
        });
    }

    /// Adds the child task that parks until the worker count is raised.
    fn add_sentinel(self: &Arc<Self>, set: &mut WorkerSet) {
        let inner = self.clone();
        set.spawn(async move {
            inner.await_worker_count_raise().await;
            Ok(GroupOutcome::Sentinel)
        });
    }

    /// Suspends until `set_worker_count` raises the target or the download
    /// finishes, or the task is cancelled (so no pause path can strand the
    /// drain loop behind a sentinel nobody will wake). Returns immediately if
    /// any of them already happened.
    async fn await_worker_count_raise(&self) {
        loop {
            // Registered before the flags are read, so a signal in between
            // cannot be missed.
            let notified = self.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.lock();
                if state.wake_closed {
                    return;
                }
                if state.pending_wake {
                    state.pending_wake = false;
                    return;
                }
            }
            tokio::select! {
                () = &mut notified => {}
                () = self.cancel.cancelled() => return,
            }
        }
    }

    fn signal_wake(&self) {
        {
            let mut state = self.lock();
            if state.wake_closed {
                return;
            }
            // If the sentinel is parked it sees this on waking; if it has not
            // parked yet (or has not been added back), it must not park on a
            // raise that already happened.
            state.pending_wake = true;
        }
        self.wake.notify_waiters();
    }

    fn close_wake(&self, state: &mut State) {
        state.wake_closed = true;
        state.pending_wake = false;
        self.wake.notify_waiters();
    }

    /// One worker: claim a gap (or steal one — see `claim_next`), stream it
    /// to disk, repeat until the file is done or this worker is retired by
    /// a lowered target count.
    async fn worker_loop(self: &Arc<Self>, index: usize) -> Result<(), DownloadError> {
        let _cleanup = WorkerCleanup {
            inner: self.clone(),
            index,
        };
        crate::engine_log!(
            "{} started",
            crate::worker_tag(self.id, &self.file_name(), index)
        );

        loop {
            let claim = {
                let mut state = self.lock();
                if state.should_retire(index) {
                    return Ok(());
                }
                match state.claim_next(index) {
                    Some(claim) => claim,
                    None => return Ok(()),
                }
            };
            self.download(claim, index).await.inspect_err(|error| {
                let _ = error;
                crate::engine_log!(
                    "{} failed: {error:?}",
                    crate::worker_tag(self.id, &self.file_name(), index)
                );
            })?;
            let mut state = self.lock();
            state.reserved.remove(&index);
            state.write_cursor.remove(&index);
        }
    }

    async fn download(&self, claim: ByteRange, index: usize) -> Result<(), DownloadError> {
        let url = self
            .lock()
            .effective_url
            .clone()
            .ok_or_else(|| DownloadError::InvalidUrl {
                url: self.source_url.as_str().to_string(),
            })?;
        crate::engine_log!(
            "{} requesting url={} range={}-{}",
            crate::worker_tag(self.id, &self.file_name(), index),
            self.source_url,
            claim.start,
            claim.end
        );
        // A cancelled worker stops exactly as one whose file has gone does:
        // it falls through to the length check below with whatever it wrote.
        let fetched = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => None,
            fetched = self.transport.fetch(RangeRequest::new(url, Some(claim))) => Some(fetched),
        };

        let mut offset = claim.start;
        if let Some(fetched) = fetched {
            let response = fetched?;
            if !(200..300).contains(&response.status_code) {
                return Err(self.status_error(response.status_code));
            }
            // A `200` to a ranged request carries the whole body from byte 0.
            // That is only usable by the worker whose claim starts at 0 (it
            // stops at its claim end, as for a Range-ignoring origin); at any
            // other offset the bytes would land in the wrong place while the
            // file still reached the right size.
            if claim.start > 0 && response.status_code != 206 {
                return Err(DownloadError::RangeNotHonored {
                    status: response.status_code,
                    start: claim.start,
                });
            }

            let mut body = response.body;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = self.cancel.cancelled() => break,
                    next = body.next() => next,
                };
                let Some(chunk) = next else { break };
                let chunk = chunk?;

                let mut state = self.lock();
                // The live end of this claim, not the one captured when the
                // request was issued: another worker may have stolen this
                // claim's unwritten tail since (`claim_next`), shrinking
                // `reserved[worker]`. Reading it fresh each iteration is what
                // lets this worker notice and stop exactly at the new
                // boundary — the same mechanism a Range-ignoring origin's
                // whole-body response already relies on to stop at the
                // original claim end.
                let effective_end = state.reserved.get(&index).map_or(claim.end, |r| r.end);
                if offset >= effective_end {
                    break;
                }
                // `file` goes `None` the instant `pause()` runs. Checking it
                // and writing under the same lock is what makes the write
                // and the recording one step: a byte is folded into
                // `completed` (via `record`) only if this exact write to
                // disk just succeeded on the open file, and `pause()` can
                // only run before or after, never inside.
                let Some(file) = state.file.as_ref() else {
                    break;
                };
                let writable = (chunk.len() as i64).min(effective_end - offset);
                // Written straight from the received buffer, no extra copy.
                file.write(&chunk[..writable as usize], offset)?;
                self.record(&mut state, ByteRange::new(offset, offset + writable));
                offset += writable;
                state.write_cursor.insert(index, offset);
            }
        }

        // The stream ended cleanly (no error) but didn't cover the whole
        // (possibly shrunk) claim — a truncated body. Bytes written so far
        // are already on disk and folded into `completed` above; make the
        // short read a defined failure instead of silently re-handing the
        // same gap back to `next_claim` forever. Reading the end live again
        // here — rather than reusing `effective_end` from inside the loop —
        // is what tells a genuine truncation (offset short of the current
        // live end) apart from a clean stop at a boundary another worker
        // just stole (offset exactly at the current live end).
        let final_end = self
            .lock()
            .reserved
            .get(&index)
            .map_or(claim.end, |r| r.end);
        if offset < final_end {
            return Err(DownloadError::TruncatedResponse {
                expected: final_end - claim.start,
                received: offset - claim.start,
            });
        }
        crate::engine_log!(
            "{} downloading done: {} bytes",
            crate::worker_tag(self.id, &self.file_name(), index),
            offset - claim.start
        );
        Ok(())
    }

    /// Folds a freshly written range into the completed set, checkpointing
    /// the sidecar every `checkpoint_interval` bytes.
    fn record(&self, state: &mut State, range: ByteRange) {
        state.completed.insert(range);
        #[cfg(debug_assertions)]
        state.write_log.push(range);
        state.bytes_since_checkpoint += range.length();
        if state.bytes_since_checkpoint >= state.configuration.checkpoint_interval {
            self.checkpoint(state, false);
        }
    }

    /// Writes the sidecar describing what has been written, and — about once
    /// a minute, or when `force_sync` — flushes the file first.
    ///
    /// The sidecar records two sets: `completed`, everything written, and
    /// `durable_completed`, what had been `fsync`ed. Recording only the first
    /// without flushing would be unsafe: the sidecar is replaced with a temp
    /// file plus a rename, which macOS orders aggressively — so under power
    /// loss or a kernel panic it can land while the `pwrite`s it describes
    /// have not. The next launch would then trust a `RangeSet` covering
    /// zero-filled blocks; `prepare()`'s backing-file size check passes (the
    /// file was truncated to full size at creation), and the download
    /// finalizes silently corrupt. `ResumeSidecar::resumable_ranges` is what
    /// keeps that from happening while still flushing rarely: after a
    /// restart only the flushed set is trusted.
    ///
    /// Flushing on *every* checkpoint, as this used to, was measured at
    /// about as much time as writing the data itself, and it blocks every
    /// worker of the file while it runs.
    ///
    /// Failures used to be swallowed. A sidecar that silently fails to write
    /// means a crash loses all progress with no signal anywhere, so the
    /// reason is recorded in `last_checkpoint_failure` and surfaced through
    /// the engine's snapshot to the UI.
    fn checkpoint(&self, state: &mut State, force_sync: bool) {
        state.bytes_since_checkpoint = 0;
        state.ticks_since_checkpoint = 0;
        // Spec §5.3: no Range support means no checkpointing. A sidecar
        // written here would later be trusted by `prepare()`'s resume path
        // (were it not itself gated on `accepts_ranges`) and hand `claim_next`
        // a non-zero-start claim against an origin whose responses always
        // start at byte 0 — corrupting the file. Simplest to never write the
        // sidecar in the first place, rather than rely solely on the
        // resume-side guard.
        if !state.accepts_ranges {
            return;
        }
        // Same guard the callers already apply: no descriptor means there is
        // nothing durable for a sidecar to describe.
        let Some(file) = state.file.as_ref() else {
            return;
        };

        let result: Result<(), String> = (|| {
            if force_sync || state.ticks_since_sync >= state.configuration.sync_interval_ticks {
                file.sync().map_err(|error| error.to_string())?;
                state.durable = state.completed.clone();
                state.ticks_since_sync = 0;
            }
            let mut sidecar = ResumeSidecar::new(
                self.source_url.clone(),
                state.effective_url.as_ref().map(SourceUrl::from_url),
                state.total_bytes,
                state.validator.clone(),
                state.completed.clone(),
            )
            .with_durable_completed(state.durable.clone());
            if let Some(boot_session_id) = &state.configuration.boot_session_id {
                sidecar = sidecar.with_boot_session_id(boot_session_id.clone());
            }
            sidecar
                .save(&self.sidecar_path)
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(()) => state.last_checkpoint_failure = None,
            Err(reason) => {
                state.checkpoint_failure_count += 1;
                state.last_checkpoint_failure = Some(format!(
                    "Could not save resume state for {}: {reason}",
                    self.file_name()
                ));
            }
        }
    }
}

impl State {
    fn has_work_remaining(&self) -> bool {
        self.next_free_claim().is_some() || self.stealable_remainder().is_some()
    }

    fn next_free_claim(&self) -> Option<ByteRange> {
        let reserved: Vec<ByteRange> = self.reserved.values().copied().collect();
        self.completed.next_claim(self.total_bytes, &reserved)
    }

    /// Retires the highest-indexed workers first, so lowering the target from
    /// 100 to 3 keeps three long-lived workers rather than churning all of
    /// them.
    fn should_retire(&self, index: usize) -> bool {
        let rank = self
            .live_worker_indices
            .iter()
            .position(|live| *live == index)
            .unwrap_or(0);
        rank >= self.target_worker_count
    }

    /// Hands `index` its next unit of work: a free gap if one exists,
    /// otherwise a slice stolen from whichever busy worker has the largest
    /// unwritten remainder. Spec §5.1: there should be no unclaimed,
    /// unreserved bytes sitting idle while a worker wants work — a fresh
    /// free gap is claimed whole (`RangeSet::next_claim`), and once every
    /// remaining byte is already reserved, further parallelism comes
    /// entirely from splitting the biggest claim still in flight.
    fn claim_next(&mut self, index: usize) -> Option<ByteRange> {
        if let Some(claim) = self.next_free_claim() {
            self.reserved.insert(index, claim);
            return Some(claim);
        }

        let (victim, remainder) = self.stealable_remainder()?;
        let mid = remainder.start + remainder.length() / 2;
        if let Some(victim_claim) = self.reserved.get(&victim).copied() {
            // Shrinks the victim's own claim in place; its `download` loop
            // reads this live on every iteration and stops exactly at `mid`
            // instead of continuing into bytes this new claim now owns. The
            // victim's in-flight request is never touched — no restart, just
            // an early stop once its region ends.
            self.reserved
                .insert(victim, ByteRange::new(victim_claim.start, mid));
        }
        let claim = ByteRange::new(mid, remainder.end);
        self.reserved.insert(index, claim);
        Some(claim)
    }

    /// The largest unwritten remainder of any other busy worker's claim,
    /// provided it is large enough to be worth splitting — same floor
    /// `next_claim` used to apply to fresh gaps, now applied only where
    /// splitting actually happens. `None` when nothing is stealable, either
    /// because every busy worker is nearly done or because the origin
    /// doesn't support `Range` (splitting a single non-ranged claim would
    /// hand the new claim a non-zero start against an origin whose
    /// responses always begin at byte 0, corrupting the file).
    fn stealable_remainder(&self) -> Option<(usize, ByteRange)> {
        if !self.accepts_ranges {
            return None;
        }
        let mut best: Option<(usize, ByteRange)> = None;
        for (other, claim) in &self.reserved {
            let cursor = self.write_cursor.get(other).copied().unwrap_or(claim.start);
            if cursor >= claim.end {
                continue;
            }
            let remainder = ByteRange::new(cursor, claim.end);
            if remainder.length() <= self.configuration.min_chunk * 2 {
                continue;
            }
            if best.is_none_or(|(_, best_remainder)| remainder.length() > best_remainder.length()) {
                best = Some((*other, remainder));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::FakeOrigin;

    fn task() -> DownloadTask {
        DownloadTask::new(
            Uuid::new_v4(),
            SourceUrl::new("https://example.com/file.bin").unwrap(),
            std::env::temp_dir().join("sdm-never-written.bin"),
            Arc::new(FakeOrigin::new(vec![0; 16])),
            DownloadTaskConfiguration::new(2, 4, 128, None),
        )
    }

    /// The hang behind C2-F1: a `pause()` that lands after the target reset
    /// and before the spawn leaves a sentinel parked with no worker to wake
    /// it. The sentinel must give up as soon as the task is cancelled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_sentinel_returns_once_the_task_is_cancelled() {
        let task = task();
        task.inner.pause();
        tokio::time::timeout(
            Duration::from_secs(10),
            task.inner.await_worker_count_raise(),
        )
        .await
        .expect("the sentinel stayed parked after a pause");
    }

    /// A pause that arrives before `run_workers` is a no-op for it: nothing
    /// is spawned, and it returns instead of waiting on a sentinel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_workers_after_a_pause_spawns_nothing_and_returns() {
        let task = task();
        task.inner.pause();
        tokio::time::timeout(Duration::from_secs(10), task.inner.run_workers())
            .await
            .expect("run_workers hung after a pause")
            .unwrap();
        assert_eq!(task.inner.lock().peak_live_worker_count, 0);
    }
}
