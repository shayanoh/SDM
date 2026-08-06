import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func reconcileShrinksTheLargestWorkerPoolFirstUnderTheGlobalConnectionCap() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(200_000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 2,
            segmentsPerItem: 1,
            globalMaxConnections: 3,
            maxConnectionsPerHost: 100,
            downloadFolder: dir
        )
    )

    let itemA = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let itemB = DownloadItem(url: URL(string: "https://example.com/b.bin")!, filename: "b.bin")
    await engine.setSegmentCount(4, for: itemA.id)
    await engine.setSegmentCount(1, for: itemB.id)
    await engine.add(DownloadPackage(name: "Batch", items: [itemA, itemB]))

    var spins = 0
    while true {
        let aSettled = await snapshotItem(itemA.id, in: engine)?.activeSegments == 2
        let bSettled = await snapshotItem(itemB.id, in: engine)?.activeSegments == 1
        if aSettled && bSettled || spins >= 100_000 { break }
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)
    #expect(await snapshotItem(itemA.id, in: engine)?.activeSegments == 2)
    #expect(await snapshotItem(itemB.id, in: engine)?.activeSegments == 1)

    await gate.open()
    try await engine.runUntilIdle()
}

@Test func perHostCapAppliesEvenWhenTheGlobalCapHasRoom() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(200_000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 32,
            maxConnectionsPerHost: 2,
            downloadFolder: dir
        )
    )

    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.setSegmentCount(4, for: item.id)
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    var spins = 0
    while await snapshotItem(item.id, in: engine)?.activeSegments != 2, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await gate.open()
    try await engine.runUntilIdle()
}
