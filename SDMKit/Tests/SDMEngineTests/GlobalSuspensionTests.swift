import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func globallySuspendedHoldsAnEnabledItemFromStarting() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin", isEnabled: true)
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    await engine.setGloballySuspended(true)

    // Give the scheduler a few ticks' worth of opportunity; it must still
    // never have started the item while suspended.
    for _ in 0..<5 { await engine.tick() }
    #expect(await snapshotItem(item.id, in: engine)?.state == .queued)

    // Crucially: the item's own `isEnabled` was never touched by the hold.
    #expect(await snapshotItem(item.id, in: engine)?.isEnabled == true)
}

@Test func liftingGlobalSuspensionLetsAlreadyEnabledItemsStart() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin", isEnabled: true)
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    await engine.setGloballySuspended(true)
    await engine.setGloballySuspended(false)

    try await engine.runUntilIdle()
    #expect(await snapshotItem(item.id, in: engine)?.state == .completed)
}

@Test func explicitStartOnOneItemLiftsTheSuspensionForEveryone() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<2).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false)
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))
    await engine.setGloballySuspended(true)

    // Starting just one item is a real user action — it should lift the
    // launch-time hold for the whole engine, not only for that one item.
    await engine.setEnabled(true, for: items[0].id)

    try await engine.runUntilIdle()
    #expect(await snapshotItem(items[0].id, in: engine)?.state == .completed)
    // The second item was never enabled, so it correctly never ran — this
    // confirms the suspension lift did not itself enable anything.
    #expect(await snapshotItem(items[1].id, in: engine)?.state == .queued)
    #expect(await snapshotItem(items[1].id, in: engine)?.isEnabled == false)
}
