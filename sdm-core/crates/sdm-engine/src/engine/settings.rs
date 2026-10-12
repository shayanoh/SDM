use std::path::PathBuf;

use sdm_core::AppTiming;

/// The engine's live-tunable settings; see `DownloadEngine::update_settings`.
///
/// The fields are public, as in Swift, but every constructor and builder
/// re-checks the preconditions Swift's initializer enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSettings {
    pub max_concurrent: usize,
    pub segments_per_item: usize,
    pub global_max_connections: usize,
    /// Spec §6.4 and §12 (default 8): enforced together with
    /// `global_max_connections` by `ConnectionAllocator`, largest pool
    /// yielding first.
    pub max_connections_per_host: usize,
    pub download_folder: PathBuf,
    /// Bytes written per worker between sidecar checkpoints (spec §4.3's byte
    /// half). Settable so a test can drive the checkpoint path without moving
    /// eight megabytes.
    pub checkpoint_interval_bytes: i64,
    /// Heartbeat ticks that must pass with no further change before durable
    /// state is written. Spec §4.2's "~2 s after the last change" debounce,
    /// expressed in ticks at `AppTiming::TICKS_PER_SECOND`.
    pub persist_debounce_ticks: usize,
    /// The floor `DownloadTask`'s stealable-remainder rule enforces when
    /// splitting a busy worker's claim to hand half to an idle one: a split
    /// only happens when both resulting halves would still be at least this
    /// large. An unclaimed free gap is always handed out whole regardless of
    /// size — this only bounds *splitting*, not claiming. Settings UI default
    /// is 10 MB, exposed there in 1...100 MB.
    pub min_segment_size_bytes: i64,
    /// Heartbeat ticks between checks that each completed item's file is
    /// still on disk (`ItemSnapshot::file_missing`). A file deleted outside
    /// SDM is noticed within this long; it used to be a `stat` per completed
    /// item on every snapshot.
    pub file_check_interval_ticks: usize,
}

impl EngineSettings {
    /// Settings with Swift's defaults for everything but the four required
    /// arguments: 8 connections per host, an 8 MiB checkpoint interval, a
    /// two-second persist debounce, a 10 MiB minimum segment and a
    /// five-second file check. Each is replaceable with the matching
    /// `with_*` builder.
    ///
    /// # Panics
    ///
    /// Panics unless `max_concurrent`, `segments_per_item` and
    /// `global_max_connections` are at least 1 (Swift's `precondition`s).
    pub fn new(
        max_concurrent: usize,
        segments_per_item: usize,
        global_max_connections: usize,
        download_folder: PathBuf,
    ) -> Self {
        let settings = Self {
            max_concurrent,
            segments_per_item,
            global_max_connections,
            max_connections_per_host: 8,
            download_folder,
            checkpoint_interval_bytes: 8 * 1024 * 1024,
            persist_debounce_ticks: AppTiming::TICKS_PER_SECOND * 2,
            min_segment_size_bytes: 10 * 1024 * 1024,
            file_check_interval_ticks: AppTiming::TICKS_PER_SECOND * 5,
        };
        settings.validate();
        settings
    }

    pub fn with_max_connections_per_host(mut self, max_connections_per_host: usize) -> Self {
        self.max_connections_per_host = max_connections_per_host;
        self.validate();
        self
    }

    pub fn with_checkpoint_interval_bytes(mut self, checkpoint_interval_bytes: i64) -> Self {
        self.checkpoint_interval_bytes = checkpoint_interval_bytes;
        self.validate();
        self
    }

    pub fn with_persist_debounce_ticks(mut self, persist_debounce_ticks: usize) -> Self {
        self.persist_debounce_ticks = persist_debounce_ticks;
        self.validate();
        self
    }

    pub fn with_min_segment_size_bytes(mut self, min_segment_size_bytes: i64) -> Self {
        self.min_segment_size_bytes = min_segment_size_bytes;
        self.validate();
        self
    }

    pub fn with_file_check_interval_ticks(mut self, file_check_interval_ticks: usize) -> Self {
        self.file_check_interval_ticks = file_check_interval_ticks;
        self.validate();
        self
    }

    /// The fields are public, so `DownloadEngine` checks them again when it
    /// takes the settings.
    pub(crate) fn validate(&self) {
        assert!(self.max_concurrent >= 1, "maxConcurrent must be at least 1");
        assert!(
            self.segments_per_item >= 1,
            "segmentsPerItem must be at least 1"
        );
        assert!(
            self.global_max_connections >= 1,
            "globalMaxConnections must be at least 1"
        );
        assert!(
            self.max_connections_per_host >= 1,
            "maxConnectionsPerHost must be at least 1"
        );
        assert!(
            self.checkpoint_interval_bytes > 0,
            "checkpointIntervalBytes must be positive"
        );
        assert!(
            self.persist_debounce_ticks >= 1,
            "persistDebounceTicks must be at least 1"
        );
        assert!(
            self.min_segment_size_bytes > 0,
            "minSegmentSizeBytes must be positive"
        );
        assert!(
            self.file_check_interval_ticks >= 1,
            "fileCheckIntervalTicks must be at least 1"
        );
    }
}
