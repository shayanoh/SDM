import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func item(
    total: Int64? = 1000,
    done: Int64 = 0,
    state: ItemState = .stopped,
    isEnabled: Bool = true,
    bytesPerSecond: Double = 0,
    history: [Double] = []
) -> ItemSnapshot {
    ItemSnapshot(
        id: UUID(),
        url: URL(string: "https://example.com/file")!,
        filename: "file",
        totalBytes: total,
        completed: done > 0 ? RangeSet([ByteRange(start: 0, end: done)]) : RangeSet(),
        state: state,
        isEnabled: isEnabled,
        isResumable: true,
        activeSegments: 0,
        configuredSegments: 4,
        bytesPerSecond: bytesPerSecond,
        speedHistory: history
    )
}

private func package(_ items: [ItemSnapshot]) -> PackageSnapshot {
    PackageSnapshot(id: UUID(), name: "p", priority: .normal, items: items)
}

// MARK: - averageRecentSpeed / estimatedSecondsRemaining

@Test func averageRecentSpeedFallsBackWhenThereIsNoHistory() {
    #expect(averageRecentSpeed(of: [], fallback: 42) == 42)
}

@Test func averageRecentSpeedUsesOnlyTheTrailingEtaWindow() {
    // Older samples than the window must not drag the average.
    let history =
        [Double](repeating: 9999, count: 10)
        + [Double](repeating: 100, count: LiveWindow.etaTicks)
    #expect(averageRecentSpeed(of: history, fallback: 0) == 100)
}

@Test func averageRecentSpeedAveragesAShortHistoryOverItsOwnLength() {
    #expect(averageRecentSpeed(of: [100, 300], fallback: 0) == 200)
}

@Test func estimatedSecondsRemainingIsZeroWithoutSpeedOrTotal() {
    #expect(estimatedSecondsRemaining(totalBytes: 1000, completedBytes: 0, speed: 0) == 0)
    #expect(estimatedSecondsRemaining(totalBytes: 0, completedBytes: 0, speed: 100) == 0)
}

@Test func estimatedSecondsRemainingDividesTheRemainderBySpeed() {
    #expect(estimatedSecondsRemaining(totalBytes: 1000, completedBytes: 400, speed: 100) == 6)
}

// MARK: - PackageAggregate

@Test func packageAggregateOfNoItemsIsEmpty() {
    #expect(PackageAggregate(items: [], globalRecentSpeed: 100) == PackageAggregate())
}

@Test func packageAggregateSumsBytesAndSpeedAcrossItems() {
    let aggregate = PackageAggregate(
        items: [
            item(total: 1000, done: 250, state: .running, bytesPerSecond: 10),
            item(total: 500, done: 500, state: .completed),
            item(total: nil, done: 30, state: .running, bytesPerSecond: 5),
        ], globalRecentSpeed: 0)
    #expect(aggregate.completedBytes == 780)
    #expect(aggregate.totalBytes == 1500)
    #expect(aggregate.bytesPerSecond == 15)
}

@Test func packageAggregateAgreesWithPackageSnapshotsOwnAggregates() {
    let items = [
        item(total: 1000, done: 250, state: .running, bytesPerSecond: 10, history: [1, 2, 3]),
        item(total: 700, done: 100, state: .queued, history: [5]),
    ]
    let snapshot = package(items)
    let aggregate = PackageAggregate(items: items, globalRecentSpeed: 0)
    #expect(aggregate.completedBytes == snapshot.completedBytes)
    #expect(aggregate.totalBytes == snapshot.totalBytes)
    #expect(aggregate.bytesPerSecond == snapshot.bytesPerSecond)
    #expect(aggregate.speedHistory == snapshot.bytesPerSecondHistory)
}

@Test func packageAggregateHistoryAlignsShorterHistoriesOnTheTrailingEdge() {
    let aggregate = PackageAggregate(
        items: [item(history: [1, 2, 3, 4]), item(history: [10, 20]), item()],
        globalRecentSpeed: 0)
    #expect(aggregate.speedHistory == [1, 2, 13, 24])
}

@Test func packageAggregateHistoryKeepsOnlyTheSparklineWindow() {
    let window = LiveWindow.packageSparklineSamples
    let long = (0..<(window + 100)).map(Double.init)
    let items = [item(history: long), item(history: [1000])]
    let aggregate = PackageAggregate(items: items, globalRecentSpeed: 0)
    // Exactly the tail of what the uncapped sum would have been.
    #expect(aggregate.speedHistory == Array(package(items).bytesPerSecondHistory.suffix(window)))
    #expect(aggregate.speedHistory.count == window)
}

@Test func packageAggregateHasNoEtaWhenNothingIsRunning() {
    let aggregate = PackageAggregate(
        items: [item(total: 1000, state: .queued), item(total: 1000, state: .stopped)],
        globalRecentSpeed: 100)
    #expect(aggregate.eta == 0)
}

@Test func packageAggregateEtaIsTheSlowestRunningItemPlusTheQueuedRemainder() {
    let aggregate = PackageAggregate(
        items: [
            // 600 left at 100 B/s → 6 s
            item(total: 1000, done: 400, state: .running, history: [100]),
            // 100 left at 50 B/s → 2 s
            item(total: 200, done: 100, state: .running, history: [50]),
            // 800 queued at the global 200 B/s → 4 s
            item(total: 1000, done: 200, state: .queued),
            // Unknown size: contributes nothing rather than a negative.
            item(total: nil, done: 50, state: .queued),
            item(total: 5000, state: .stopped),
        ], globalRecentSpeed: 200)
    #expect(aggregate.eta == 10)
}

@Test func packageAggregateRunningItemWithoutHistoryFallsBackToItsCurrentSpeed() {
    let aggregate = PackageAggregate(
        items: [item(total: 1000, done: 0, state: .running, bytesPerSecond: 250)],
        globalRecentSpeed: 0)
    #expect(aggregate.eta == 4)
}

// MARK: - EngineFigures

@Test func engineFiguresOfNoPackagesAreZero() {
    #expect(EngineFigures(packages: []) == EngineFigures())
}

@Test func engineFiguresCountRunningAndQueued() {
    let figures = EngineFigures(packages: [
        package([item(state: .running), item(state: .queued), item(state: .queued)]),
        package([item(state: .running), item(state: .stopped), item(state: .failed(reason: "x"))]),
    ])
    #expect(figures.runningCount == 2)
    #expect(figures.queuedCount == 2)
}

@Test func engineFiguresRingAveragesRunningQueuedAndCompletedItemsOfKnownSize() {
    let figures = EngineFigures(packages: [
        package([
            item(total: 1000, done: 500, state: .running),  // 0.5
            item(total: 200, done: 200, state: .completed),  // 1.0
            item(total: 400, done: 0, state: .queued),  // 0.0
            // Not ring states, or no known size: excluded entirely.
            item(total: 1000, done: 900, state: .stopped),
            item(total: 1000, done: 900, state: .failed(reason: "x")),
            item(total: nil, done: 10, state: .running),
            item(total: 0, state: .running),
        ])
    ])
    #expect(figures.overallFraction == 0.5)
    #expect(figures.ringCompletedBytes == 700)
    #expect(figures.ringTotalBytes == 1600)
}

@Test func engineFiguresWaitingCountsEnabledUnfinishedItemsAndTheirPackages() {
    let figures = EngineFigures(packages: [
        package([item(state: .stopped), item(state: .completed)]),
        package([item(state: .completed), item(state: .queued, isEnabled: false)]),
        package([item(state: .failed(reason: "x")), item(state: .running)]),
    ])
    #expect(figures.waitingItemCount == 3)
    #expect(figures.waitingPackageCount == 2)
}
