import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func item(
    _ name: String,
    priority: Priority? = nil,
    position: Int = 0,
    enabled: Bool = true,
    resumable: Bool? = true,
    state: ItemState = .queued
) -> DownloadItem {
    DownloadItem(
        url: URL(string: "https://example.com/\(name)")!,
        filename: name,
        totalBytes: 1000,
        state: state,
        isEnabled: enabled,
        isResumable: resumable,
        priority: priority,
        position: position
    )
}

private func package(
    _ name: String,
    priority: Priority = .normal,
    position: Int = 0,
    items: [DownloadItem]
) -> DownloadPackage {
    DownloadPackage(name: name, items: items, priority: priority, position: position)
}

@Test func runningSetNeverExceedsMaxConcurrent() {
    let items = (0..<10).map { item("f\($0).bin", position: $0) }
    let input = SchedulerInput(
        packages: [package("p", items: items)],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 3
    )
    #expect(Scheduler.desiredRunningSet(input).count == 3)
}

@Test func higherPriorityWinsRegardlessOfPosition() {
    let low = item("low.bin", position: 0)
    let high = item("high.bin", priority: .highest, position: 9)
    let input = SchedulerInput(
        packages: [package("p", items: [low, high])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [high.id])
}

@Test func packagePriorityLiftsItsItems() {
    let normal = item("a.bin", position: 0)
    let lifted = item("b.bin", position: 0)
    let input = SchedulerInput(
        packages: [
            package("normal", priority: .normal, position: 0, items: [normal]),
            package("urgent", priority: .highest, position: 1, items: [lifted]),
        ],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [lifted.id])
}

@Test func itemPriorityOverridesItsPackage() {
    let pinned = item("pinned.bin", priority: .highest, position: 5)
    let sibling = item("other.bin", position: 0)
    let input = SchedulerInput(
        packages: [package("low", priority: .lowest, items: [sibling, pinned])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [pinned.id])
}

@Test func disabledItemsAreNeverScheduled() {
    let disabled = item("off.bin", position: 0, enabled: false)
    let enabled = item("on.bin", position: 1)
    let input = SchedulerInput(
        packages: [package("p", items: [disabled, enabled])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 5
    )
    #expect(Scheduler.desiredRunningSet(input) == [enabled.id])
}

@Test func completedAndFailedItemsAreNeverScheduled() {
    let done = item("done.bin", position: 0, state: .completed)
    let failed = item("bad.bin", position: 1, state: .failed(reason: "404"))
    let ready = item("go.bin", position: 2)
    let input = SchedulerInput(
        packages: [package("p", items: [done, failed, ready])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 5
    )
    #expect(Scheduler.desiredRunningSet(input) == [ready.id])
}

@Test func runningNonResumableItemKeepsItsSlotAgainstHigherPriority() {
    let stubborn = item("noresume.bin", priority: .lowest, position: 9, resumable: false)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [stubborn, urgent])],
        runningNow: [stubborn.id],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [stubborn.id])
}

@Test func nonResumableItemThatIsNotRunningHasNoSpecialClaim() {
    let stubborn = item("noresume.bin", priority: .lowest, position: 9, resumable: false)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [stubborn, urgent])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [urgent.id])
}

@Test func hysteresisProtectsRecentlyStartedItems() {
    let fresh = item("fresh.bin", priority: .lowest, position: 9)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [fresh, urgent])],
        runningNow: [fresh.id],
        startedRecently: [fresh.id],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [fresh.id])
}

@Test func settledRunningItemIsPreemptedByHigherPriority() {
    let settled = item("settled.bin", priority: .lowest, position: 9)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [settled, urgent])],
        runningNow: [settled.id],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [urgent.id])
}

@Test func reservationsAreCappedByMaxConcurrent() {
    let a = item("a.bin", position: 0, resumable: false)
    let b = item("b.bin", position: 1, resumable: false)
    let c = item("c.bin", position: 2, resumable: false)
    let input = SchedulerInput(
        packages: [package("p", items: [a, b, c])],
        runningNow: [a.id, b.id, c.id],
        startedRecently: [],
        maxConcurrent: 2
    )
    #expect(Scheduler.desiredRunningSet(input).count == 2)
}

@Test func rankOrdersByPriorityThenPackageThenPosition() {
    let input = [
        package(
            "second",
            priority: .normal,
            position: 1,
            items: [item("b1.bin", position: 1), item("b0.bin", position: 0)]
        ),
        package("first", priority: .normal, position: 0, items: [item("a0.bin", position: 0)]),
        package("urgent", priority: .high, position: 2, items: [item("z.bin", position: 0)]),
    ]
    #expect(Scheduler.rank(input).map(\.filename) == ["z.bin", "a0.bin", "b0.bin", "b1.bin"])
}
