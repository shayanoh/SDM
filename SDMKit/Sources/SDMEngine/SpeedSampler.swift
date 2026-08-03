/// Per-item speed measurement, driven by the engine's 1 Hz tick.
///
/// Package and global speeds are sums over these samplers rather than separate
/// state, so the three figures can never disagree. See spec §5.4.
public struct SpeedSampler: Sendable {
    private let historyLength: Int
    private let smoothingFactor: Double
    private var pendingBytes: Int64 = 0
    private var smoothed: Double = 0
    private var samples: [Double] = []

    /// - Parameter smoothingFactor: EMA weight for the newest sample, in `(0, 1]`.
    ///   `1.0` disables smoothing.
    public init(historyLength: Int = 60, smoothingFactor: Double = 0.4) {
        precondition(historyLength > 0, "historyLength must be positive")
        precondition(
            smoothingFactor > 0 && smoothingFactor <= 1,
            "smoothingFactor must be in (0, 1], got \(smoothingFactor)"
        )
        self.historyLength = historyLength
        self.smoothingFactor = smoothingFactor
    }

    /// Adds bytes transferred since the last tick.
    public mutating func record(bytes: Int64) {
        precondition(bytes >= 0, "bytes must be non-negative")
        pendingBytes += bytes
    }

    /// Closes the current one-second window.
    public mutating func tick() {
        let raw = Double(pendingBytes)
        pendingBytes = 0
        smoothed = smoothingFactor * raw + (1 - smoothingFactor) * smoothed
        samples.append(raw)
        if samples.count > historyLength {
            samples.removeFirst(samples.count - historyLength)
        }
    }

    public var bytesPerSecond: Double { smoothed }
    public var history: [Double] { samples }

    public var runningAverage: Double {
        guard !samples.isEmpty else { return 0 }
        return samples.reduce(0, +) / Double(samples.count)
    }
}
