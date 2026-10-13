import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// "Resume downloads automatically on launch" used to be its own in-memory
/// suspension flag (`isGloballySuspended`), because the old single-axis model
/// had no real `.stopped` state to durably rest on — the only way to hold an
/// item back was `isEnabled`, which belongs to the user. Now that `.stopped`
/// is a first-class, correctly-persisted state, the durable store never
/// claims anything is in flight or about to run, and "auto start on launch"
/// is nothing but `resumeAll()` called once after `restore()`.

/// `persist()` squashes `.queued`/`.running` to `.stopped` in what actually
/// reaches the store, even while the live engine reports the real state.
@Test func persistedSnapshotNeverContainsQueuedOrRunning() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = InMemoryStateStore()
    let gate = Gate()
    let engine = DownloadEngine(
        transport: GatedOrigin(payload: testPayload(4000), gate: gate, phase: .body),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    await gate.waitForArrival()

    // The live engine genuinely has it running...
    #expect(await snapshotItem(item.id, in: engine)?.state == .running)
    // ...but nothing written to the store ever says so.
    #expect(await store.load().packages.first?.items.first?.state == .stopped)

    await gate.open()
    try await engine.runUntilIdle()
}

/// A store seeded directly (as a crash or an older build might leave one)
/// with `.queued`/`.running` items is normalized to `.stopped` on restore,
/// same as `persist()` would have written it.
@Test func restoreNormalizesLegacyQueuedAndRunningToStopped() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var package = DownloadPackage(
        name: "Batch",
        items: [
            DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin"),
            DownloadItem(url: URL(string: "https://example.com/b.bin")!, filename: "b.bin"),
        ]
    )
    package.items[0].state = .running
    package.items[1].state = .queued

    let store = InMemoryStateStore()
    await store.save(PersistedState(packages: [package]))

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    await engine.restore()

    let restored = await engine.snapshot().packages.first?.items ?? []
    #expect(restored.allSatisfy { $0.state == .stopped })
}

/// The whole point of the redesign: "resume on launch" is just `resumeAll()`
/// after `restore()`, and it respects the same enabled/disabled split every
/// other Resume All call does — no separate suspension mechanism, no magic
/// "starting one item resumes everyone."
@Test func resumeAllAfterRestoreReproducesAutoStartOnLaunch() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(10)
    var package = DownloadPackage(
        name: "Batch",
        items: [
            DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin"),
            DownloadItem(url: URL(string: "https://example.com/b.bin")!, filename: "b.bin"),
        ]
    )
    package.items[0].state = .stopped
    package.items[1].state = .stopped
    package.items[1].isEnabled = false

    let store = InMemoryStateStore()
    await store.save(PersistedState(packages: [package]))

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    await engine.restore()

    // Auto-start off: nothing here calls resumeAll, so both items sit put.
    for _ in 0..<3 { await engine.tick() }
    #expect(await snapshotItem(package.items[0].id, in: engine)?.state == .stopped)
    #expect(await snapshotItem(package.items[1].id, in: engine)?.state == .stopped)

    // Auto-start on: the app layer's equivalent is one resumeAll() call.
    await engine.resumeAll()
    try await engine.runUntilIdle()

    #expect(await snapshotItem(package.items[0].id, in: engine)?.state == .completed)
    // Disabled item never resumed, even though Resume All ran.
    #expect(await snapshotItem(package.items[1].id, in: engine)?.state == .stopped)
    #expect(await snapshotItem(package.items[1].id, in: engine)?.isEnabled == false)
}
