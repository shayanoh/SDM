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
    /// Field arithmetic rather than a call into the core: rows read this on
    /// every tick. `SDMShimTests` pins it to the core's `rangeSetTotalBytes`.
    public var totalBytes: Int64 { ranges.reduce(0) { $0 + ($1.end - $1.start) } }
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
    /// The core's default policy. There is no meaningful fallback if the
    /// core cannot answer, so that is fatal.
    public static let `default`: QualityPreferences = {
        do { return try qualityPreferencesDefault() } catch {
            fatalError("the Rust core could not provide default quality preferences: \(error)")
        }
    }()
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
        (try? mediaContainerFileExtension(container: self)) ?? "bin"
    }
}

extension ItemSnapshot {
    // Both are field arithmetic rather than calls into the core: every row
    // reads them on every tick, and a call would send the whole snapshot
    // across. `SDMShimTests` pins each to the core's own answer.

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
