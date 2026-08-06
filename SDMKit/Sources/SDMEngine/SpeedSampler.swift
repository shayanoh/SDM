import SDMCore

/// Per-item speed measurement, driven by the engine's heartbeat tick
/// (`AppTiming.ticksPerSecond` Hz). See spec §5.4.
///
/// Reports a **rolling average of the last two real seconds** of transferred
/// bytes while active — not an exponential moving average, so the displayed
/// number and the sparkline agree about what "current speed" means — and
/// **snaps to zero immediately** via `idle()` once the item stops actually
/// transferring, rather than decaying toward zero over several more ticks.
public struct SpeedSampler: Sendable {
    private let historyLength: Int
    private let averagingWindowTicks: Int
    private let ticksPerSecond: Int
    private var pendingBytes: Int64 = 0
    private var samples: [Double] = []
    /// Trailing window used only for `bytesPerSecond`, separate from the
    /// full `samples` history kept for the sparkline. Cleared by `idle()` so
    /// resuming after an idle stretch reports the new tick's rate outright,
    /// rather than averaging it against the zero samples idle time left
    /// behind.
    private var activeSamples: [Double] = []
    private var isActive = false

    /// - Parameters:
    ///   - historyLength: sparkline sample count kept, in ticks. Defaults to
    ///     60 seconds' worth at the current `AppTiming.ticksPerSecond`.
    ///   - averagingWindowSeconds: how many trailing seconds `bytesPerSecond`
    ///     averages over while active. Spec calls for "the last two
    ///     seconds".
    ///   - ticksPerSecond: injected rather than read from `AppTiming`
    ///     directly, so a test can exercise a different rate deterministically
    ///     without depending on the shared constant's current value.
    public init(
        historyLength: Int = AppTiming.ticksPerSecond * 60,
        averagingWindowSeconds: Double = 2,
        ticksPerSecond: Int = AppTiming.ticksPerSecond
    ) {
        precondition(historyLength > 0, "historyLength must be positive")
        precondition(averagingWindowSeconds > 0, "averagingWindowSeconds must be positive")
        precondition(ticksPerSecond > 0, "ticksPerSecond must be positive")
        self.historyLength = historyLength
        self.ticksPerSecond = ticksPerSecond
        self.averagingWindowTicks = Swift.max(
            1, Int(averagingWindowSeconds * Double(ticksPerSecond)))
    }

    /// Adds bytes transferred since the last tick.
    public mutating func record(bytes: Int64) {
        precondition(bytes >= 0, "bytes must be non-negative")
        pendingBytes += bytes
    }

    /// Whether bytes have been `record`ed since the last `tick()`/`idle()`
    /// closed a window. Lets a caller give a just-stopped item one final
    /// real `tick()` (folding in a pause/completion-moment burst) even if it
    /// stopped running before the caller's own tick loop ever observed it as
    /// running — a short-lived download can go `.queued` → `.running` →
    /// `.completed` entirely between two heartbeat ticks.
    public var hasPendingBytes: Bool { pendingBytes > 0 }

    /// Closes the current tick's window for an item that is actively
    /// transferring. A tick covers `1 / ticksPerSecond` seconds, so the raw
    /// byte count is scaled up to a bytes/second estimate before it is
    /// appended to history.
    public mutating func tick() {
        isActive = true
        let bytesPerSecondEstimate = Double(pendingBytes) * Double(ticksPerSecond)
        pendingBytes = 0
        append(bytesPerSecondEstimate)
        activeSamples.append(bytesPerSecondEstimate)
        if activeSamples.count > averagingWindowTicks {
            activeSamples.removeFirst(activeSamples.count - averagingWindowTicks)
        }
    }

    /// Closes the window for an item that is not currently running
    /// (stopped, queued, completed, failed). Reports zero on the very next
    /// read — no decay — and appends a zero sample to history so the
    /// sparkline visibly drops rather than freezing at its last value.
    ///
    /// A no-op if the sampler is already idle. The caller (`DownloadEngine
    /// .tick()`) has no way to know it already reported the drop, so it
    /// calls this every heartbeat for every non-running item indefinitely —
    /// without this guard, `history` (and therefore this whole value) kept
    /// changing shape for a full `historyLength` worth of ticks after any
    /// item went idle, which defeated equality-based change detection one
    /// layer up (`EngineController` comparing telemetry snapshots to decide
    /// whether to republish, and hence whether a `List` row needs to
    /// re-render at all).
    public mutating func idle() {
        guard isActive else { return }
        isActive = false
        pendingBytes = 0
        append(0)
        activeSamples.removeAll()
    }

    private mutating func append(_ value: Double) {
        samples.append(value)
        if samples.count > historyLength {
            samples.removeFirst(samples.count - historyLength)
        }
    }

    /// Mean of the trailing `averagingWindowSeconds` of samples since the
    /// sampler last went active — a running average, not an EMA. Zero
    /// immediately whenever the sampler is not active.
    public var bytesPerSecond: Double {
        guard isActive, !activeSamples.isEmpty else { return 0 }
        return activeSamples.reduce(0, +) / Double(activeSamples.count)
    }

    public var history: [Double] { samples }

    public var runningAverage: Double {
        guard !samples.isEmpty else { return 0 }
        return samples.reduce(0, +) / Double(samples.count)
    }
}
