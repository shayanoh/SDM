import Foundation

// Thin adapters over the generated bindings. Each one forwards to an exported
// Rust function or is a one-line accessor; no domain logic lives here.

/// Single source of truth for the engine's heartbeat rate (owned by Rust).
public enum AppTiming {
    public static let ticksPerSecond: Int = (try? appTimingTicksPerSecond()) ?? 5
}

/// The mean of the trailing ETA-window samples of `history`, `fallback` when
/// the history is empty. A failed call (a Rust panic) degrades to `fallback`.
public func averageRecentSpeed(of history: [Double], fallback: Double) -> Double {
    (try? averageRecentSpeed(history: history, fallback: fallback)) ?? fallback
}

extension ByteRange {
    public var length: Int64 { end - start }
    public var isEmpty: Bool { end == start }
}

extension RangeSetData {
    public init() { self.init(ranges: []) }
    /// Zero if the core rejects the set (it cannot, for one the core produced).
    public var totalBytes: Int64 { (try? rangeSetTotalBytes(set: self)) ?? 0 }
    /// Adds `range`, merged with whatever it touches (the merge is Rust's).
    /// A rejected range (negative or inverted) leaves the set unchanged.
    public mutating func insert(_ range: ByteRange) {
        if let merged = try? rangeSetInserting(set: self, range: range) { self = merged }
    }
    public func gaps(within total: Int64) -> [ByteRange] {
        (try? rangeSetGapsWithin(set: self, total: total)) ?? []
    }
}

extension QualityPreferences {
    public static let `default` = QualityPreferences(
        maxHeight: 1080, videoCodecs: [.av1, .vp9, .h264], containers: [.mp4, .webm],
        audioCodecs: [.opus, .aac])
}

extension EngineSnapshot {
    /// What the app shows before the first heartbeat.
    public static let empty = EngineSnapshot(
        packages: [], globalBytesPerSecond: 0, globalHistory: [],
        figures: EngineFigures(
            runningCount: 0, queuedCount: 0, overallFraction: 0, ringCompletedBytes: 0,
            ringTotalBytes: 0, waitingItemCount: 0, waitingPackageCount: 0),
        generation: 0)
}

extension FormatChoice {
    public var isWholesale: Bool { wholesaleSelector != nil }
}

extension MediaContainer {
    public var fileExtension: String {
        switch self {
        case .mp4: "mp4"
        case .webm: "webm"
        case .m4a: "m4a"
        case .other(let ext): ext
        }
    }
}

extension ItemSnapshot {
    /// A `.queued` item that has already failed at least once.
    public var isRetrying: Bool { state == .queued && failedAttemptCount != nil }
    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(total)
    }
}

/// The built-in themes, sorted by name (owned by Rust).
public enum ThemeCatalog {
    /// Empty if the call fails; the theme store then falls back to its defaults.
    public static func builtInThemes() -> [Theme] { (try? themeCatalog()) ?? [] }
}
