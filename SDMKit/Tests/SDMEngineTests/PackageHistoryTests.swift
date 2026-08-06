import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func makeItemSnapshot(speedHistory: [Double]) -> ItemSnapshot {
    ItemSnapshot(
        id: UUID(),
        url: URL(string: "https://example.com/a.bin")!,
        filename: "a.bin",
        totalBytes: nil,
        completed: RangeSet(),
        state: .running,
        isEnabled: true,
        isResumable: nil,
        activeSegments: 1,
        configuredSegments: 1,
        bytesPerSecond: speedHistory.last ?? 0,
        speedHistory: speedHistory
    )
}

/// Items can join mid-run with shorter histories (a later addition, or a
/// sampler that has ticked fewer times); the sum aligns every array's most
/// recent sample to the same trailing edge, front-padding the shorter ones
/// with zero rather than misaligning by index.
@Test func packageHistorySumsMemberItemHistoriesAlignedToTheirTrailingEdge() {
    let a = makeItemSnapshot(speedHistory: [10, 20, 30])
    let b = makeItemSnapshot(speedHistory: [1, 2])
    let package = PackageSnapshot(id: UUID(), name: "P", priority: .normal, items: [a, b])
    #expect(package.bytesPerSecondHistory == [10, 21, 32])
}

@Test func packageHistoryOfNoItemsIsEmpty() {
    let package = PackageSnapshot(id: UUID(), name: "P", priority: .normal, items: [])
    #expect(package.bytesPerSecondHistory.isEmpty)
}
