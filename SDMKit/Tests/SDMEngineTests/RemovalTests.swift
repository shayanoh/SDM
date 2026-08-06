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

@Test func resetDownloadRestartsFromZero() async throws {
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

    // `resetDownload` reconciles immediately, so with nothing else competing
    // for the single concurrency slot the item can already be `.running` (or
    // even done, against a zero-latency fake origin) by the time this reads
    // back — the property under test is that it restarted from zero, not the
    // exact state at this instant.
    try await engine.runUntilIdle()
    let reset = await snapshotItem(item.id, in: engine)
    #expect(reset?.state == .completed)
    #expect(reset?.completed.totalBytes == 10)
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

@Test func setEnabledForAllItemsTogglesEveryItem() async throws {
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
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false)
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))

    await engine.setEnabledForAllItems(true)
    var snapshot = await engine.snapshot()
    #expect(snapshot.packages[0].items.allSatisfy { $0.isEnabled })

    await engine.setEnabledForAllItems(false)
    snapshot = await engine.snapshot()
    #expect(snapshot.packages[0].items.allSatisfy { !$0.isEnabled })
}
