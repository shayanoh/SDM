import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// `EngineSettings.minSegmentSizeBytes` is the user-facing floor; it is
/// threaded straight into `DownloadTask.Configuration.minChunk`, whose own
/// splitting behavior is already covered at the `DownloadTask` level (see
/// `ResegmentationTests`). These tests instead prove the *engine* actually
/// passes the configured value through, rather than a hardcoded constant, by
/// observing how many segments a single item is allowed to split into.
private func activeSegments(for itemID: UUID, in engine: DownloadEngine) async -> Int {
    await snapshotItem(itemID, in: engine)?.activeSegments ?? 0
}

@Test func minSegmentSizeBytesPreventsSplittingBelowTheFloor() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let payload = testPayload(8000)
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: payload, gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 32,
            downloadFolder: dir, minSegmentSizeBytes: Int64(payload.count))
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    // A floor as large as the whole file means splitting the sole worker's
    // claim for any of the other three configured segments would drop below
    // it, so they must never actually claim anything, no matter how long we
    // wait.
    for _ in 0..<50 { await Task.yield() }
    #expect(await activeSegments(for: item.id, in: engine) == 1)

    await gate.open()
    try await engine.runUntilIdle()
}

@Test func minSegmentSizeBytesAllowsSplittingAboveTheFloor() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let payload = testPayload(8000)
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: payload, gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 32,
            downloadFolder: dir, minSegmentSizeBytes: 64)
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    var spins = 0
    while await activeSegments(for: item.id, in: engine) < 4, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await gate.open()
    try await engine.runUntilIdle()
}

@Test func defaultMinSegmentSizeIsTenMegabytes() {
    let settings = EngineSettings(
        maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 1,
        downloadFolder: URL(fileURLWithPath: "/tmp"))
    #expect(settings.minSegmentSizeBytes == 10 * 1024 * 1024)
}
