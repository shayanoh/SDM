import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func removeItemWithoutDeletingFileKeepsBytesOnDisk() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    #expect(FileManager.default.fileExists(atPath: destination.path))

    await engine.removeItem(item.id, deleteFile: false)

    #expect(FileManager.default.fileExists(atPath: destination.path))
    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.isEmpty)
}

@Test func removeItemDeletingFileTrashesItAndDropsTheEmptyPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    #expect(FileManager.default.fileExists(atPath: destination.path))

    await engine.removeItem(item.id, deleteFile: true)

    #expect(!FileManager.default.fileExists(atPath: destination.path))
    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.isEmpty)
}

@Test func removePackageDeletingFilesTrashesTheWholeFolder() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<2).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin")
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))
    try await engine.runUntilIdle()

    let folder = dir.appendingPathComponent("Batch")
    #expect(FileManager.default.fileExists(atPath: folder.path))

    await engine.removePackage(packageID, deleteFiles: true)

    #expect(!FileManager.default.fileExists(atPath: folder.path))
    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.isEmpty)
}

/// A reset must not itself start a download — it only discards progress.
/// Resetting a `.completed` item has neither a "stays queued" nor a "stays
/// stopped" prior state to preserve, so it lands `.stopped` like a `.failed`
/// one would: the bytes are gone and nothing should pick it back up until the
/// operator explicitly says so.
@Test func resetDownloadDiscardsBytesAndDoesNotAutoStart() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    #expect(await snapshotItem(item.id, in: engine)?.state == .completed)

    await engine.resetDownload(item.id)

    let reset = await snapshotItem(item.id, in: engine)
    #expect(reset?.state == .stopped)
    #expect(reset?.completed.totalBytes == 0)
    #expect(reset?.totalBytes == nil)

    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    #expect(!FileManager.default.fileExists(atPath: destination.path))
}

@Test func resetDownloadPreservesAQueuedItemsSchedulingState() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    // A single concurrency slot, held indefinitely by `running` — `target`
    // can never be promoted off `.queued` for the whole test, regardless of
    // scheduling order, which is what makes this deterministic.
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(10), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let running = DownloadItem(url: URL(string: "https://example.com/running.bin")!, filename: "running.bin")
    let target = DownloadItem(url: testSourceURL, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [running, target]))
    #expect(await snapshotItem(target.id, in: engine)?.state == .queued)

    await engine.resetDownload(target.id)
    #expect(await snapshotItem(target.id, in: engine)?.state == .queued)

    await gate.open()
    try await engine.runUntilIdle()
}

@Test func resetDownloadPreservesAStoppedItemsSchedulingState() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin", state: .stopped)
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    #expect(await snapshotItem(item.id, in: engine)?.state == .stopped)

    await engine.resetDownload(item.id)

    #expect(await snapshotItem(item.id, in: engine)?.state == .stopped)
}

@Test func resetDownloadOnADisabledItemStaysStopped() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    var item = DownloadItem(url: testSourceURL, filename: "a.bin", state: .queued)
    item.isEnabled = false
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    await engine.resetDownload(item.id)

    let reset = await snapshotItem(item.id, in: engine)
    #expect(reset?.state == .stopped)
    #expect(reset?.isEnabled == false)
}

@Test func reorderPackagesAppliesTheGivenOrder() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let packageIDs = (0..<3).map { _ in UUID() }
    for (index, id) in packageIDs.enumerated() {
        await engine.add(DownloadPackage(id: id, name: "P\(index)"))
    }

    let newOrder = [packageIDs[2], packageIDs[0], packageIDs[1]]
    await engine.reorderPackages(newOrder)

    let ordered = await engine.snapshot().packages.map(\.id)
    #expect(ordered == newOrder)
}

@Test func fileMissingIsTrueOnlyWhenACompletedItemsFileIsGone() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == false)

    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    try FileManager.default.removeItem(at: destination)

    #expect(await snapshotItem(item.id, in: engine)?.fileMissing == true)
}

/// `pauseAll()`/`resumeAll()` are defined as "select every item and click
/// Stop/Start" — nothing more. In particular neither touches `isEnabled`, so
/// a user-disabled item stays stopped through a full pause/resume cycle
/// (Resume All is not a backdoor around Disable) while an enabled sibling
/// gets queued again.
@Test func pauseAllAndResumeAllNeverTouchIsEnabled() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = Gate()
    let engine = makeGatedEngine(payload: testPayload(10), folder: dir, gate: gate, phase: .body)
    let items = (0..<2).map {
        DownloadItem(url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin")
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))
    await gate.waitForArrival()

    // One item explicitly disabled by the user before the blanket pause.
    await engine.setEnabled(false, for: items[1].id)

    await engine.pauseAll()
    var snapshot = await engine.snapshot()
    #expect(snapshot.packages[0].items.allSatisfy { $0.state == .stopped })
    #expect(snapshot.packages[0].items[0].isEnabled == true)
    #expect(snapshot.packages[0].items[1].isEnabled == false)

    await engine.resumeAll()
    snapshot = await engine.snapshot()
    // The enabled item is queued again; the disabled one stays stopped —
    // Resume All is not a backdoor around Disable.
    #expect(snapshot.packages[0].items[0].state == .queued)
    #expect(snapshot.packages[0].items[1].state == .stopped)
    #expect(snapshot.packages[0].items[1].isEnabled == false)

    await gate.open()
    try await engine.runUntilIdle()
}

/// Disabling a running item stops it immediately (not merely marks it
/// ineligible for the next reconcile), and re-enabling it does not itself
/// resume it — the user still has to `startItem`.
@Test func disablingARunningItemStopsItAndEnablingAloneDoesNotResumeIt() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(1000)
    let gate = Gate()
    let engine = makeGatedEngine(payload: payload, folder: dir, gate: gate, phase: .body)
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    let itemID = package.items[0].id
    await engine.add(package)
    await gate.waitForArrival()

    // Disabling returns immediately — it does not wait for the gated worker
    // to actually unwind (that would hang for as long as the gate stays
    // closed, defeating the point of a user-facing Stop). The item's `state`
    // still flips to `.stopped` synchronously either way.
    await engine.setEnabled(false, for: itemID)
    #expect(await snapshotItem(itemID, in: engine)?.state == .stopped)

    await engine.setEnabled(true, for: itemID)
    #expect(await snapshotItem(itemID, in: engine)?.state == .stopped)

    await engine.startItem(itemID)
    #expect(await snapshotItem(itemID, in: engine)?.state == .queued)

    // The old (cancelled, but not yet unwound) runner is still occupying the
    // item's slot in `runners`, so nothing new starts until it actually
    // finishes — which only happens once the gate opens. `runUntilIdle()`
    // drains that old runner first, reconciles again, and only then starts
    // the fresh attempt that reaches completion.
    await gate.open()
    try await engine.runUntilIdle()
    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
}
