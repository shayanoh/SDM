import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func remainingAttemptsIsNilWhenAnItemHasNeverFailed() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    await engine.add(package)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(package.items[0].id, in: engine)?.remainingAttempts == nil)
}

@Test func remainingAttemptsCountsDownAfterATransientFailure() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 500
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        retryPolicy: RetryPolicy(maxAttempts: 3, baseDelay: .seconds(30))
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    await engine.add(package)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(package.items[0].id, in: engine)?.remainingAttempts == 2)
    #expect(await snapshotItem(package.items[0].id, in: engine)?.state == .queued)
}

@Test func retryOnAFailedItemResumesItAfterTheOriginRecovers() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 404
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    guard case .failed = await snapshotItem(itemID, in: engine)?.state else {
        Issue.record("expected the item to be terminally failed after a 404")
        return
    }

    await origin.setBehavior(FakeOrigin.Behavior())
    await engine.retry(itemID)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
    #expect(
        try Data(
            contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
        ) == testPayload(1000)
    )
}

@Test func retryOnAnyNonFailedItemIsANoOp() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    await engine.retry(itemID)
    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
}
