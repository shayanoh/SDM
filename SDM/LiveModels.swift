import Foundation
import Observation
import SDMCore
import SDMEngine

// MARK: - ItemLiveModel

/// One item's "hot" figures — the ones that change every heartbeat tick
/// while it transfers. One stable object per item, so Observation tracks
/// each item separately: a row reading *its* model is invalidated only when
/// that item's own numbers change, not whenever any item's do. (The
/// `[UUID: ItemTelemetry]` dictionary this replaced was a single observable
/// property, so one active download re-rendered every row's telemetry
/// leaves on every tick.)
///
/// Every setter is gated on an actual change: `@Observable` notifies on
/// assignment, not on value change.
@MainActor
@Observable
final class ItemLiveModel {
    let id: UUID
    private(set) var completed: RangeSet
    private(set) var completedBytes: Int64
    private(set) var totalBytes: Int64?
    private(set) var activeSegments: Int
    private(set) var configuredSegments: Int
    private(set) var bytesPerSecond: Double
    private(set) var speedHistory: [Double]
    /// `speedHistory` averaged over the ETA window, maintained here so no
    /// view has to re-reduce the history in its `body`.
    private(set) var recentAverageSpeed: Double
    /// Seconds left on a retry backoff hold. Here rather than in the row's
    /// structural content because it counts down every second on its own —
    /// treating it as structural would hand the `List` a new data source
    /// once a second for as long as anything is retrying.
    private(set) var retryHoldSeconds: Int?

    init(_ item: ItemSnapshot) {
        retryHoldSeconds = item.retryHoldSeconds
        id = item.id
        completed = item.completed
        completedBytes = item.completed.totalBytes
        totalBytes = item.totalBytes
        activeSegments = item.activeSegments
        configuredSegments = item.configuredSegments
        bytesPerSecond = item.bytesPerSecond
        speedHistory = item.speedHistory
        recentAverageSpeed = averageRecentSpeed(
            of: item.speedHistory, fallback: item.bytesPerSecond)
    }

    /// Copies whichever figures differ from `item`. Returns whether any did,
    /// so the caller knows the owning package's aggregate is stale.
    @discardableResult
    func update(from item: ItemSnapshot) -> Bool {
        // Not part of `changed`: no package aggregate depends on it.
        if retryHoldSeconds != item.retryHoldSeconds {
            retryHoldSeconds = item.retryHoldSeconds
        }
        var changed = false
        if completed != item.completed {
            completed = item.completed
            let bytes = item.completed.totalBytes
            if completedBytes != bytes { completedBytes = bytes }
            changed = true
        }
        if totalBytes != item.totalBytes {
            totalBytes = item.totalBytes
            changed = true
        }
        if activeSegments != item.activeSegments {
            activeSegments = item.activeSegments
            changed = true
        }
        if configuredSegments != item.configuredSegments {
            configuredSegments = item.configuredSegments
            changed = true
        }
        var speedInputsChanged = false
        if bytesPerSecond != item.bytesPerSecond {
            bytesPerSecond = item.bytesPerSecond
            speedInputsChanged = true
        }
        if speedHistory != item.speedHistory {
            speedHistory = item.speedHistory
            speedInputsChanged = true
        }
        if speedInputsChanged {
            let average = averageRecentSpeed(
                of: item.speedHistory, fallback: item.bytesPerSecond)
            if recentAverageSpeed != average { recentAverageSpeed = average }
            changed = true
        }
        return changed
    }
}

// MARK: - PackageLiveModel

/// The observable holder for one package's `PackageAggregate`. See
/// `ItemLiveModel` for why this is one object per package.
@MainActor
@Observable
final class PackageLiveModel {
    let id: UUID
    private(set) var aggregate: PackageAggregate
    /// Whether the package had a running item when last refreshed.
    /// Bookkeeping for `EngineController.refreshLiveModels`, not something a
    /// view reads — hence not observed.
    @ObservationIgnored var hasRunningItems = false

    init(id: UUID, aggregate: PackageAggregate) {
        self.id = id
        self.aggregate = aggregate
    }

    func update(_ newAggregate: PackageAggregate) {
        if aggregate != newAggregate { aggregate = newAggregate }
    }
}

// MARK: - EngineStats

/// Whole-engine figures, updated once per tick. Each property is observed —
/// and gated — separately, so a view reading only `runningCount` is not
/// invalidated by `overallFraction` creeping up every tick.
@MainActor
@Observable
final class EngineStats {
    private(set) var runningCount = 0
    private(set) var queuedCount = 0
    private(set) var overallFraction: Double = 0
    private(set) var ringCompletedBytes: Int64 = 0
    private(set) var ringTotalBytes: Int64 = 0
    private(set) var waitingItemCount = 0
    private(set) var waitingPackageCount = 0
    private(set) var globalBytesPerSecond: Double = 0
    private(set) var globalHistory: [Double] = []
    /// `globalHistory` averaged over the ETA window — the rate a queued item
    /// is assumed to download at.
    private(set) var recentGlobalSpeed: Double = 0

    func update(from snapshot: EngineSnapshot) {
        let figures = EngineFigures(packages: snapshot.packages)
        if runningCount != figures.runningCount { runningCount = figures.runningCount }
        if queuedCount != figures.queuedCount { queuedCount = figures.queuedCount }
        if overallFraction != figures.overallFraction {
            overallFraction = figures.overallFraction
        }
        if ringCompletedBytes != figures.ringCompletedBytes {
            ringCompletedBytes = figures.ringCompletedBytes
        }
        if ringTotalBytes != figures.ringTotalBytes { ringTotalBytes = figures.ringTotalBytes }
        if waitingItemCount != figures.waitingItemCount {
            waitingItemCount = figures.waitingItemCount
        }
        if waitingPackageCount != figures.waitingPackageCount {
            waitingPackageCount = figures.waitingPackageCount
        }
        if globalBytesPerSecond != snapshot.globalBytesPerSecond {
            globalBytesPerSecond = snapshot.globalBytesPerSecond
        }
        if globalHistory != snapshot.globalHistory {
            globalHistory = snapshot.globalHistory
        }
        let recent = averageRecentSpeed(
            of: snapshot.globalHistory, fallback: snapshot.globalBytesPerSecond)
        if recentGlobalSpeed != recent { recentGlobalSpeed = recent }
    }
}
