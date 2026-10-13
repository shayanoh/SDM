import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// A running item that has not yet been probed (`isResumable == nil`) is not
/// protected by the scheduler's pass 1 (`isResumable == false` only), so this
/// isolates hysteresis specifically: without it, a higher-priority item added
/// a moment later would preempt A instantly.
@Test func recentlyStartedRunningItemsSurviveAPreemptingAdditionUntilTheHysteresisWindowElapses()
    async throws
{
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )

    let itemA = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [itemA]))

    var spins = 0
    while await snapshotItem(itemA.id, in: engine)?.state != .running, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    let itemB = DownloadItem(
        url: URL(string: "https://example.com/b.bin")!,
        filename: "b.bin",
        priority: .highest
    )
    await engine.add(DownloadPackage(name: "Batch", items: [itemB]))

    // Still inside the hysteresis window: A keeps its slot despite B outranking it.
    #expect(await snapshotItem(itemA.id, in: engine)?.state == .running)
    #expect(await snapshotItem(itemB.id, in: engine)?.state == .queued)

    for _ in 0..<(AppTiming.ticksPerSecond * 5) { await engine.tick() }

    // Window elapsed: B now preempts A.
    #expect(await snapshotItem(itemA.id, in: engine)?.state == .queued)
    #expect(await snapshotItem(itemB.id, in: engine)?.state == .running)

    await gate.open()
    try await engine.runUntilIdle()
}
