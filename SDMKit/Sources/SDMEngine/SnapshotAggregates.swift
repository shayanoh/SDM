import Foundation
import SDMCore

/// Sample-window sizes shared by the live models and the views drawing them.
public enum LiveWindow {
    /// How many trailing ticks an ETA's speed is averaged over: ten seconds.
    public static let etaTicks = AppTiming.ticksPerSecond * 10
    /// How many trailing samples a package header's summed sparkline keeps.
    /// The header draws it 60 pt wide and `Sparkline` takes one sample per
    /// point, so anything beyond that is never drawn.
    public static let packageSparklineSamples = 64
}

/// Mean of the trailing `LiveWindow.etaTicks` samples, or `fallback` when
/// there is no history yet.
public func averageRecentSpeed(of history: [Double], fallback: Double) -> Double {
    guard !history.isEmpty else { return fallback }
    let window = history.suffix(LiveWindow.etaTicks)
    return window.reduce(0, +) / Double(window.count)
}

/// Seconds left at `speed`, or 0 when it cannot be estimated (no speed, or
/// an unknown total). 0 is what every caller already treats as "show
/// nothing".
public func estimatedSecondsRemaining(totalBytes: Int64, completedBytes: Int64, speed: Double)
    -> TimeInterval
{
    guard speed > 0, totalBytes > 0 else { return 0 }
    return Double(totalBytes - completedBytes) / speed
}

/// A package's summed figures, computed once per tick by the app's
/// controller rather than in each header's `body`. Same derived-not-stored
/// rule as `PackageSnapshot`'s own aggregates (spec
/// §5.4): always recomputed from the member items, never tracked
/// separately, so it cannot disagree with them.
public struct PackageAggregate: Equatable, Sendable {
    public var completedBytes: Int64 = 0
    public var totalBytes: Int64 = 0
    public var bytesPerSecond: Double = 0
    /// The member items' speed histories summed, aligned on their trailing
    /// edge, and capped at `LiveWindow.packageSparklineSamples`.
    public var speedHistory: [Double] = []
    /// Seconds until the package finishes: the slowest running item, plus
    /// the queued remainder at the global rate. 0 when nothing is running or
    /// it cannot be estimated.
    public var eta: TimeInterval = 0

    public init() {}

    /// - Parameter globalRecentSpeed: the global download rate averaged over
    ///   the ETA window — what queued items are assumed to get.
    public init(items: [ItemSnapshot], globalRecentSpeed: Double) {
        var historyLength = 0
        var runningEta: TimeInterval = 0
        var queuedRemaining: Int64 = 0
        var hasRunning = false
        for item in items {
            let itemCompleted = item.completed.totalBytes
            let itemTotal = item.totalBytes ?? 0
            completedBytes += itemCompleted
            totalBytes += itemTotal
            bytesPerSecond += item.bytesPerSecond
            historyLength = max(
                historyLength,
                min(item.speedHistory.count, LiveWindow.packageSparklineSamples))
            switch item.state {
            case .running:
                hasRunning = true
                let speed = averageRecentSpeed(
                    of: item.speedHistory, fallback: item.bytesPerSecond)
                runningEta = max(
                    runningEta,
                    estimatedSecondsRemaining(
                        totalBytes: itemTotal, completedBytes: itemCompleted, speed: speed))
            case .queued:
                if itemTotal > 0 { queuedRemaining += itemTotal - itemCompleted }
            case .stopped, .completed, .failed:
                break
            }
        }

        if historyLength > 0 {
            var summed = [Double](repeating: 0, count: historyLength)
            for item in items where !item.speedHistory.isEmpty {
                let window = item.speedHistory.suffix(historyLength)
                let padding = historyLength - window.count
                for (offset, value) in window.enumerated() {
                    summed[padding + offset] += value
                }
            }
            speedHistory = summed
        }

        if hasRunning {
            let queuedEta =
                globalRecentSpeed > 0
                ? Double(queuedRemaining) / globalRecentSpeed : 0
            eta = runningEta + queuedEta
        }
    }
}

/// The whole-engine counts every chrome surface used to re-derive for itself
/// by flat-mapping every item — sidebar badge, overall ring, menu bar icon,
/// Touch Bar. One pass, pure, over a snapshot.
public struct EngineFigures: Equatable, Sendable {
    public var runningCount = 0
    public var queuedCount = 0
    /// Mean completion of every running/queued/completed item with a known
    /// size — the "overall progress" ring.
    public var overallFraction: Double = 0
    /// Downloaded / total bytes over that same set of items.
    public var ringCompletedBytes: Int64 = 0
    public var ringTotalBytes: Int64 = 0
    /// Enabled items that are not completed yet, and how many packages hold
    /// at least one.
    public var waitingItemCount = 0
    public var waitingPackageCount = 0

    public init() {}

    public init(packages: [PackageSnapshot]) {
        var fractionSum = 0.0
        var ringCount = 0
        for package in packages {
            var packageHasWaiting = false
            for item in package.items {
                let isRingState: Bool
                switch item.state {
                case .running:
                    runningCount += 1
                    isRingState = true
                case .queued:
                    queuedCount += 1
                    isRingState = true
                case .completed:
                    isRingState = true
                case .stopped, .failed:
                    isRingState = false
                }
                if isRingState, let total = item.totalBytes, total > 0 {
                    let completed = item.completed.totalBytes
                    fractionSum += Double(completed) / Double(total)
                    ringCount += 1
                    ringCompletedBytes += completed
                    ringTotalBytes += total
                }
                if item.isEnabled && item.state != .completed {
                    waitingItemCount += 1
                    packageHasWaiting = true
                }
            }
            if packageHasWaiting { waitingPackageCount += 1 }
        }
        overallFraction = ringCount > 0 ? fractionSum / Double(ringCount) : 0
    }
}
