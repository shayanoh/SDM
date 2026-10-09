import Foundation
import SDMCore
import Testing

@testable import SDMEngine

// `snapshot()` hands back the previous snapshot — or the previous value of an
// unchanged package — instead of rebuilding it. These pin the property that
// makes that safe: a change made through any public mutation is visible in
// the very next snapshot, and nothing else is.

private func makeEngine(
    dir: URL, segmentsPerItem: Int = 2, fileCheckIntervalTicks: Int = 1
) -> DownloadEngine {
    DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: segmentsPerItem, globalMaxConnections: 8,
            downloadFolder: dir, fileCheckIntervalTicks: fileCheckIntervalTicks)
    )
}

/// Two stopped items in two packages: nothing runs, so the engine is idle.
private func addIdlePackages(to engine: DownloadEngine) async -> (first: UUID, second: UUID) {
    let first = DownloadItem(url: testSourceURL, filename: "a.bin", state: .stopped)
    let second = DownloadItem(url: testSourceURL, filename: "b.bin", state: .stopped)
    await engine.add(DownloadPackage(name: "One", items: [first]))
    await engine.add(DownloadPackage(name: "Two", items: [second]))
    return (first.id, second.id)
}

@Test func idleEngineSnapshotsStayEqualAcrossTicks() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    _ = await addIdlePackages(to: engine)

    let before = await engine.snapshot()
    for _ in 0..<10 { await engine.tick() }
    #expect(await engine.snapshot() == before)
}

@Test func snapshotReflectsEnablementChangedAfterAnIdleSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    let ids = await addIdlePackages(to: engine)

    #expect(await snapshotItem(ids.first, in: engine)?.isEnabled == true)
    await engine.setEnabled(false, for: ids.first)
    #expect(await snapshotItem(ids.first, in: engine)?.isEnabled == false)
    // And the untouched item is exactly as it was.
    #expect(await snapshotItem(ids.second, in: engine)?.isEnabled == true)
}

@Test func snapshotReflectsASegmentOverrideSetAfterAnIdleSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir, segmentsPerItem: 2)
    let ids = await addIdlePackages(to: engine)

    #expect(await snapshotItem(ids.first, in: engine)?.configuredSegments == 2)
    await engine.setSegmentCount(5, for: ids.first)
    #expect(await snapshotItem(ids.first, in: engine)?.configuredSegments == 5)
    #expect(await snapshotItem(ids.second, in: engine)?.configuredSegments == 2)
}

@Test func snapshotReflectsNewSettingsAppliedAfterAnIdleSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir, segmentsPerItem: 2)
    let ids = await addIdlePackages(to: engine)

    #expect(await snapshotItem(ids.first, in: engine)?.configuredSegments == 2)
    await engine.updateSettings(
        EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 7, globalMaxConnections: 8, downloadFolder: dir))
    #expect(await snapshotItem(ids.first, in: engine)?.configuredSegments == 7)
}

@Test func snapshotReflectsRemovalAfterAnIdleSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    let ids = await addIdlePackages(to: engine)

    #expect(await engine.snapshot().packages.count == 2)
    await engine.removeItem(ids.first, deleteFile: false)
    let after = await engine.snapshot()
    #expect(after.packages.flatMap(\.items).map(\.id) == [ids.second])
}

@Test func snapshotReflectsADownloadStartedAndFinishedAfterAnIdleSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    let ids = await addIdlePackages(to: engine)

    #expect(await snapshotItem(ids.first, in: engine)?.state == .stopped)
    await engine.startItem(ids.first)
    try await engine.runUntilIdle()

    let finished = await snapshotItem(ids.first, in: engine)
    #expect(finished?.state == .completed)
    #expect(finished?.completed.totalBytes == 10)
    #expect(await snapshotItem(ids.second, in: engine)?.state == .stopped)
}

// MARK: - File-missing cadence

@Test func missingFileIsNoticedOnlyWhenTheCheckIntervalComesRound() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir, fileCheckIntervalTicks: 3)
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    // The first tick always checks; the file is there.
    await engine.tick()
    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == false)

    try FileManager.default.removeItem(
        at: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin"))

    // Two more ticks are still inside the interval: not re-checked yet.
    await engine.tick()
    await engine.tick()
    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == false)

    // The third closes it.
    await engine.tick()
    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == true)
}

@Test func resetClearsAStaleMissingFileFlagBeforeTheNextCheck() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    // A long interval, so only the first tick's check ever runs here.
    let engine = makeEngine(dir: dir, fileCheckIntervalTicks: 1000)
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()
    try FileManager.default.removeItem(
        at: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin"))
    await engine.tick()
    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == true)

    // Download it again: the freshly written file must not inherit the flag.
    await engine.resetDownload(item.id)
    await engine.startItem(item.id)
    try await engine.runUntilIdle()

    let redone = await snapshotItem(item.id, in: engine)
    #expect(redone?.state == .completed)
    #expect(redone?.fileMissing == false)
}

// MARK: - Storage reuse

/// Whether two arrays are the same storage, not merely equal — which is what
/// lets a consumer's `==` on them return in O(1).
private func sharesStorage<Element>(_ a: [Element], _ b: [Element]) -> Bool {
    a.withUnsafeBufferPointer { first in
        b.withUnsafeBufferPointer { second in
            first.baseAddress == second.baseAddress && first.count == second.count
        }
    }
}

@Test func idleEngineReturnsTheSameSnapshotStorageAcrossTicks() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    _ = await addIdlePackages(to: engine)

    // The first tick runs the one-off file check; settle past it.
    await engine.tick()
    let before = await engine.snapshot()
    for _ in 0..<10 { await engine.tick() }
    let after = await engine.snapshot()
    // A tick with nothing running must write nothing a snapshot reads.
    #expect(sharesStorage(before.packages, after.packages))
}

@Test func anUnchangedPackageKeepsItsItemStorageWhenAnotherPackageChanges() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(dir: dir)
    let ids = await addIdlePackages(to: engine)

    let before = await engine.snapshot()
    await engine.setEnabled(false, for: ids.first)
    let after = await engine.snapshot()

    #expect(!sharesStorage(before.packages[0].items, after.packages[0].items))
    #expect(after.packages[0].items[0].isEnabled == false)
    #expect(sharesStorage(before.packages[1].items, after.packages[1].items))
}
