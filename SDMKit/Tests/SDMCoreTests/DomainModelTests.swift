import Foundation
import Testing

@testable import SDMCore

@Test func prioritiesCompareByRank() {
    #expect(Priority.highest > Priority.normal)
    #expect(Priority.lowest < Priority.low)
    #expect(Priority.allCases.count == 5)
}

@Test func itemDefaultsToQueuedAndEnabled() {
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(item.state == .queued)
    #expect(item.isEnabled)
    #expect(item.priority == nil)
    #expect(item.completed.ranges.isEmpty)
    #expect(item.totalBytes == nil)
    #expect(item.isResumable == nil)
}

@Test func itemProgressIsZeroWhenSizeUnknown() {
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(item.fractionCompleted == 0)
}

@Test func itemProgressReflectsCompletedRanges() {
    var item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    item.totalBytes = 1000
    item.completed.insert(ByteRange(start: 0, end: 250))
    #expect(item.fractionCompleted == 0.25)
}

@Test func effectivePriorityFallsBackToPackage() {
    var package = DownloadPackage(name: "Season 1")
    package.priority = .high
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(package.effectivePriority(for: item) == .high)
}

@Test func itemPriorityOverridesPackage() {
    var package = DownloadPackage(name: "Season 1")
    package.priority = .low
    var item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    item.priority = .highest
    #expect(package.effectivePriority(for: item) == .highest)
}

@Test func failedStateCarriesReason() throws {
    let state = ItemState.failed(reason: "404 Not Found")
    let data = try JSONEncoder().encode(state)
    #expect(try JSONDecoder().decode(ItemState.self, from: data) == state)
}

@Test func packageRoundTripsThroughCodable() throws {
    var package = DownloadPackage(name: "Season 1")
    package.items = [
        DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    ]
    let data = try JSONEncoder().encode(package)
    #expect(try JSONDecoder().decode(DownloadPackage.self, from: data) == package)
}
