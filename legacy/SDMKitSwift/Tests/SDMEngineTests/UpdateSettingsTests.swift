import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func updateSettingsAppliesTheNewMaxConcurrentOnItsNextReconcile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<2).map {
        DownloadItem(url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin")
    }
    await engine.add(DownloadPackage(name: "Batch", items: items))

    #expect(await runningCount(in: engine) == 1)

    await engine.updateSettings(
        EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )

    #expect(await runningCount(in: engine) == 2)

    await gate.open()
    try await engine.runUntilIdle()
}

private func runningCount(in engine: DownloadEngine) async -> Int {
    await engine.snapshot().packages.flatMap(\.items).filter { $0.state == .running }.count
}
