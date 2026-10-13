import Foundation
import Testing

@testable import SDMRust

@Test func coreVersionCrossesTheBoundary() throws {
    #expect(!(try coreVersion()).isEmpty)
}

@Test func invalidArgumentsThrowInsteadOfTrapping() throws {
    #expect(throws: CoreError.self) {
        _ = try contrastRatio(hexA: "red", hexB: "#FFFFFF")
    }
}

@Test func engineRoundTripsUuidUrlAndRangeSetAcrossTheBoundary() async throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-rust-tests-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }

    let settings = EngineSettings(
        maxConcurrent: 2, segmentsPerItem: 2, globalMaxConnections: 8,
        maxConnectionsPerHost: 8, downloadFolder: directory,
        checkpointIntervalBytes: 8 * 1024 * 1024, persistDebounceTicks: 10,
        minSegmentSizeBytes: 10 * 1024 * 1024, fileCheckIntervalTicks: 25)
    let engine = try DownloadEngine(
        config: EngineConfig(
            stateFile: directory.appendingPathComponent("state.json"),
            settings: settings, binarySearchPaths: []))
    #expect(try await engine.snapshot().packages.isEmpty)

    // A URL Foundation percent-encodes: the string Swift hands over is kept.
    let url = try #require(URL(string: "https://example.com/a b.bin"))
    let component = FileComponent(
        id: UUID(), url: url, partFilename: "a b.bin", totalBytes: nil,
        completed: RangeSet(ranges: []), validator: nil, origin: .http,
        isResumable: nil, lastError: nil)
    let item = DownloadItem(
        id: UUID(), components: [component], outputFilename: "a b.bin", sourceUrl: url,
        assembly: .none, state: .queued, isEnabled: false, priority: nil, position: 0,
        metadata: nil)
    let package = DownloadPackage(
        id: UUID(), name: "Tests", items: [item], priority: .normal, position: 0, note: nil)
    try await engine.add(package: package)

    let snapshot = try await engine.snapshot()
    let echoed = try #require(snapshot.packages.first?.items.first)
    #expect(echoed.id == item.id)
    #expect(echoed.url == url)
    #expect(!echoed.isEnabled)
    #expect(echoed.completed.ranges.isEmpty)
    try await engine.shutdown()
}
