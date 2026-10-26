use sdm_core::AppTiming;

use crate::snapshot_aggregates::LiveWindow;

/// Per-item speed measurement, driven by the engine's heartbeat tick
/// (`AppTiming::TICKS_PER_SECOND` Hz). See spec §5.4.
///
/// Reports a **rolling average of the last two real seconds** of transferred
/// bytes while active — not an exponential moving average, so the displayed
/// number and the sparkline agree about what "current speed" means — and
/// **snaps to zero immediately** via [`idle`](Self::idle) once the item stops
/// actually transferring, rather than decaying toward zero over several more
/// ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedSampler {
    history_length: usize,
    averaging_window_seconds: f64,
    averaging_window_ticks: usize,
    ticks_per_second: usize,
    pending_bytes: i64,
    samples: Vec<f64>,
    /// The trailing `recent_length` of `samples`, kept as its own buffer so a
    /// snapshot can take it without slicing the long history.
    recent: Vec<f64>,
    recent_length: usize,
    /// Trailing window used only for `bytes_per_second`, separate from the
    /// full `samples` history kept for the sparkline. Cleared by `idle()` so
    /// resuming after an idle stretch reports the new tick's rate outright,
    /// rather than averaging it against the zero samples idle time left
    /// behind.
    active_samples: Vec<f64>,
    /// Whether the last window closed with `tick()` rather than `idle()`.
    /// Readable so `DownloadEngine::tick()` can leave an already-idle sampler
    /// untouched instead of calling a no-op `idle()` on it.
    is_active: bool,
    /// How many of the newest `samples` are zero in a row. An idle sampler
    /// keeps appending zeros until this covers the whole history, so a
    /// stopped item's graph slides back in time and empties instead of
    /// freezing (and overlapping the live graphs it is summed with).
    trailing_zeros: usize,
}

impl Default for SpeedSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedSampler {
    /// A sampler with Swift's initializer defaults: `history_length` of 10
    /// minutes' worth of ticks at the current `AppTiming::TICKS_PER_SECOND`,
    /// a two-second averaging window, `ticks_per_second` from `AppTiming`,
    /// and `recent_length` of `LiveWindow::ITEM_HISTORY_SAMPLES`. Each is
    /// replaceable with the matching `with_*` builder.
    pub fn new() -> Self {
        let ticks_per_second = AppTiming::TICKS_PER_SECOND;
        let averaging_window_seconds = 2.0;
        Self {
            history_length: ticks_per_second * 60 * 10,
            averaging_window_seconds,
            averaging_window_ticks: Self::window_ticks(averaging_window_seconds, ticks_per_second),
            ticks_per_second,
            pending_bytes: 0,
            samples: Vec::new(),
            recent: Vec::new(),
            recent_length: LiveWindow::ITEM_HISTORY_SAMPLES,
            active_samples: Vec::new(),
            is_active: false,
            trailing_zeros: 0,
        }
    }

    fn window_ticks(seconds: f64, ticks_per_second: usize) -> usize {
        // `Int(averagingWindowSeconds * Double(ticksPerSecond))` truncates.
        ((seconds * ticks_per_second as f64) as usize).max(1)
    }

    /// Sparkline sample count kept, in ticks.
    ///
    /// # Panics
    ///
    /// Panics if `history_length` is zero ("historyLength must be positive").
    pub fn with_history_length(mut self, history_length: usize) -> Self {
        assert!(history_length > 0, "historyLength must be positive");
        self.history_length = history_length;
        self
    }

    /// How many trailing seconds `bytes_per_second` averages over while
    /// active. Spec calls for "the last two seconds".
    ///
    /// # Panics
    ///
    /// Panics unless `seconds` is positive ("averagingWindowSeconds must be
    /// positive").
    pub fn with_averaging_window_seconds(mut self, seconds: f64) -> Self {
        assert!(seconds > 0.0, "averagingWindowSeconds must be positive");
        self.averaging_window_seconds = seconds;
        self.averaging_window_ticks = Self::window_ticks(seconds, self.ticks_per_second);
        self
    }

    /// Injected rather than read from `AppTiming` directly, so a test can
    /// exercise a different rate deterministically without depending on the
    /// shared constant's current value.
    ///
    /// # Panics
    ///
    /// Panics if `ticks_per_second` is zero ("ticksPerSecond must be
    /// positive").
    pub fn with_ticks_per_second(mut self, ticks_per_second: usize) -> Self {
        assert!(ticks_per_second > 0, "ticksPerSecond must be positive");
        self.ticks_per_second = ticks_per_second;
        self.averaging_window_ticks =
            Self::window_ticks(self.averaging_window_seconds, ticks_per_second);
        self
    }

    /// How many trailing samples `recent_history` keeps.
    ///
    /// # Panics
    ///
    /// Panics if `recent_length` is zero ("recentLength must be positive").
    pub fn with_recent_length(mut self, recent_length: usize) -> Self {
        assert!(recent_length > 0, "recentLength must be positive");
        self.recent_length = recent_length;
        self
    }

    /// Whether the last window closed with `tick()` rather than `idle()`.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Whether an idle sampler still has non-zero samples in its history, so
    /// `idle()` still has zeros to slide in. False once the graph is empty.
    pub fn is_decaying(&self) -> bool {
        self.trailing_zeros < self.samples.len()
    }

    /// Whether the window a snapshot reads (`recent_history`) still holds a
    /// non-zero sample. The full history can keep decaying after this is
    /// false without any snapshot input changing.
    pub fn recent_is_decaying(&self) -> bool {
        self.recent.iter().any(|value| *value != 0.0)
    }

    /// Adds bytes transferred since the last tick.
    ///
    /// # Panics
    ///
    /// Panics if `bytes` is negative ("bytes must be non-negative").
    pub fn record(&mut self, bytes: i64) {
        assert!(bytes >= 0, "bytes must be non-negative");
        self.pending_bytes += bytes;
    }

    /// Whether bytes have been `record`ed since the last `tick()`/`idle()`
    /// closed a window. Lets a caller give a just-stopped item one final
    /// real `tick()` (folding in a pause/completion-moment burst) even if it
    /// stopped running before the caller's own tick loop ever observed it as
    /// running — a short-lived download can go `Queued` → `Running` →
    /// `Completed` entirely between two heartbeat ticks.
    pub fn has_pending_bytes(&self) -> bool {
        self.pending_bytes > 0
    }

    /// Closes the current tick's window for an item that is actively
    /// transferring. A tick covers `1 / ticks_per_second` seconds, so the raw
    /// byte count is scaled up to a bytes/second estimate before it is
    /// appended to history.
    pub fn tick(&mut self) {
        self.is_active = true;
        let bytes_per_second_estimate = self.pending_bytes as f64 * self.ticks_per_second as f64;
        self.pending_bytes = 0;
        self.append(bytes_per_second_estimate);
        self.active_samples.push(bytes_per_second_estimate);
        if self.active_samples.len() > self.averaging_window_ticks {
            let excess = self.active_samples.len() - self.averaging_window_ticks;
            self.active_samples.drain(..excess);
        }
    }

    /// Closes the window for an item that is not currently running
    /// (stopped, queued, completed, failed). Reports zero on the very next
    /// read — no decay — and appends a zero sample to history so the
    /// sparkline visibly drops rather than freezing at its last value.
    ///
    /// While the history still holds a non-zero sample, every call slides
    /// one more zero in, so the graph of a stopped or finished item moves
    /// back in time until it is empty. A no-op after that. The caller
    /// (`DownloadEngine::tick()`) skips samplers that are neither active nor
    /// decaying, so a long-idle item changes nothing a snapshot reads.
    pub fn idle(&mut self) {
        if !self.is_active && !self.is_decaying() {
            return;
        }
        self.is_active = false;
        self.pending_bytes = 0;
        self.append(0.0);
        self.active_samples.clear();
    }

    fn append(&mut self, value: f64) {
        self.samples.push(value);
        if value == 0.0 {
            self.trailing_zeros += 1;
        } else {
            self.trailing_zeros = 0;
        }
        if self.samples.len() > self.history_length {
            let excess = self.samples.len() - self.history_length;
            self.samples.drain(..excess);
            self.trailing_zeros = self.trailing_zeros.min(self.samples.len());
        }
        self.recent.push(value);
        if self.recent.len() > self.recent_length {
            let excess = self.recent.len() - self.recent_length;
            self.recent.drain(..excess);
        }
    }

    /// Mean of the trailing `averagingWindowSeconds` of samples since the
    /// sampler last went active — a running average, not an EMA. Zero
    /// immediately whenever the sampler is not active.
    pub fn bytes_per_second(&self) -> f64 {
        if !self.is_active || self.active_samples.is_empty() {
            return 0.0;
        }
        self.active_samples.iter().fold(0.0, |sum, v| sum + v) / self.active_samples.len() as f64
    }

    pub fn history(&self) -> &[f64] {
        &self.samples
    }

    /// The trailing `recent_length` samples of `history` — what a snapshot
    /// carries per item.
    pub fn recent_history(&self) -> &[f64] {
        &self.recent
    }
}
