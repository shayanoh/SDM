import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func reorderItemsAppliesTheGivenOrderWithinAPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<3).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin", position: $0)
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))

    let newOrder = [items[2].id, items[0].id, items[1].id]
    await engine.reorderItems(newOrder, inPackage: packageID)

    let ordered = await engine.snapshot().packages[0].items.map(\.id)
    #expect(ordered == newOrder)

    try await engine.runUntilIdle()
}

@Test func moveItemRelocatesAQueuedItemIntoAnotherPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(
        url: URL(string: "https://example.com/a.bin")!, filename: "a.bin", isEnabled: false)
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [item]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))

    await engine.moveItem(item.id, toPackage: packageBID)

    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first { $0.id == packageAID }?.items.isEmpty == true)
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.map(\.id) == [item.id])
}

@Test func moveItemIsANoOpForARunningItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [item]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))

    var spins = 0
    while await snapshotItem(item.id, in: engine)?.state != .running, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await engine.moveItem(item.id, toPackage: packageBID)
    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first { $0.id == packageAID }?.items.map(\.id) == [item.id])
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.isEmpty == true)

    await gate.open()
    try await engine.runUntilIdle()
}
