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
    // Package A had only this one item, so moving it out empties A —
    // and, same as `removeItem`/`removePackage`, an emptied package is
    // dropped entirely rather than left behind as an empty husk.
    #expect(snapshot.packages.first { $0.id == packageAID } == nil)
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.map(\.id) == [item.id])
}

@Test func moveItemRelocatesTheFinishedFileToTheDestinationPackagesFolder() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(200)
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [item]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))
    try await engine.runUntilIdle()

    let sourcePath = dir.appendingPathComponent("A").appendingPathComponent("a.bin")
    let destinationPath = dir.appendingPathComponent("B").appendingPathComponent("a.bin")
    #expect(try Data(contentsOf: sourcePath) == payload)

    await engine.moveItem(item.id, toPackage: packageBID)

    #expect(!FileManager.default.fileExists(atPath: sourcePath.path))
    #expect(try Data(contentsOf: destinationPath) == payload)
    // Package A had only this one item, so moving it out empties — and
    // trashes — A's folder, same as `removeItem`/`removePackage` do.
    #expect(await engine.snapshot().packages.first { $0.id == packageAID } == nil)
    #expect(!FileManager.default.fileExists(atPath: dir.appendingPathComponent("A").path))
}

@Test func moveItemAtIndexInsertsAtThatPositionInTheDestinationPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let moving = DownloadItem(
        url: URL(string: "https://example.com/moving.bin")!, filename: "moving.bin",
        isEnabled: false)
    let existing = (0..<2).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false, position: $0)
    }
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [moving]))
    await engine.add(DownloadPackage(id: packageBID, name: "B", items: existing))

    await engine.moveItem(moving.id, toPackage: packageBID, atIndex: 1)

    let destinationItems = await engine.snapshot().packages.first { $0.id == packageBID }?.items
    #expect(destinationItems?.map(\.id) == [existing[0].id, moving.id, existing[1].id])
}

/// `moveItem` is also how same-package drag reordering works now (see
/// `PackagesListView.DraggableItemRow`, which replaced `.onMove` — the two
/// couldn't coexist on the same row). `atIndex` means "insert before
/// whatever is currently at this index in the pre-move list," which forward
/// and backward moves resolve to different post-removal insertion points
/// for — this pins both directions.
@Test func moveItemWithinTheSamePackageReordersInPlace() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<4).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false, position: $0)
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "A", items: items))

    // Drop item 0 onto item 2's row: item 0 should end up immediately
    // before wherever item 2 ends up, i.e. [1, 0, 2, 3].
    await engine.moveItem(items[0].id, toPackage: packageID, atIndex: 2)
    var ordered = await engine.snapshot().packages.first { $0.id == packageID }?.items.map(\.id)
    #expect(ordered == [items[1].id, items[0].id, items[2].id, items[3].id])

    // Drop item 3 (now last) onto item 1's row: item 3 should land
    // immediately before item 1, i.e. [items[1] pushed after].
    await engine.moveItem(items[3].id, toPackage: packageID, atIndex: 0)
    ordered = await engine.snapshot().packages.first { $0.id == packageID }?.items.map(\.id)
    #expect(ordered == [items[3].id, items[1].id, items[0].id, items[2].id])
}

/// `moveItems` is what a multi-selection drag uses (`PackagesListView`'s
/// drop handlers) — it moves an ordered batch to one contiguous run at the
/// target index in a single atomic step, rather than one `moveItem` call
/// per item.
@Test func moveItemsInsertsTheWholeBatchAsOneContiguousRunAtTheTargetIndex() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let moving = (0..<2).map {
        DownloadItem(
            url: URL(string: "https://example.com/moving\($0).bin")!, filename: "moving\($0).bin",
            isEnabled: false)
    }
    let existing = (0..<2).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false, position: $0)
    }
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: moving))
    await engine.add(DownloadPackage(id: packageBID, name: "B", items: existing))

    // Moving both A items in, dropped between B's two existing items.
    await engine.moveItems(moving.map(\.id), toPackage: packageBID, atIndex: 1)

    let destinationItems = await engine.snapshot().packages.first { $0.id == packageBID }?.items
    #expect(
        destinationItems?.map(\.id) == [existing[0].id, moving[0].id, moving[1].id, existing[1].id])
    // A had only these two items, so moving both out empties — and drops
    // — it, same as `moveItem` does for a single item.
    #expect(await engine.snapshot().packages.first { $0.id == packageAID } == nil)
}

/// The batch can straddle its own destination: some members already live
/// in the destination package, others are moving in from elsewhere — the
/// "how many of the batch sit ahead of the target index within the
/// destination itself" adjustment has to account for both at once.
@Test func moveItemsHandlesABatchStraddlingItsOwnDestination() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let fromOther = DownloadItem(
        url: URL(string: "https://example.com/other.bin")!, filename: "other.bin",
        isEnabled: false)
    let destinationItems = (0..<3).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin",
            isEnabled: false, position: $0)
    }
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [fromOther]))
    await engine.add(DownloadPackage(id: packageBID, name: "B", items: destinationItems))

    // Move item 0 (already in B) and `fromOther` (from A) to land right
    // before B's item 2 — item 0 sitting ahead of index 2 in B's own
    // pre-move list means the effective insertion point shifts back by one
    // once it's removed from its old spot.
    await engine.moveItems(
        [destinationItems[0].id, fromOther.id], toPackage: packageBID, atIndex: 2)

    let ordered = await engine.snapshot().packages.first { $0.id == packageBID }?.items.map(\.id)
    #expect(
        ordered == [
            destinationItems[1].id, destinationItems[0].id, fromOther.id, destinationItems[2].id,
        ])
    #expect(await engine.snapshot().packages.first { $0.id == packageAID } == nil)
}

/// A batch drawn from multiple distinct source packages must empty and
/// drop every one of them that ends up with no items left, not just the
/// first.
@Test func moveItemsDropsEveryEmptiedSourcePackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let itemA = DownloadItem(
        url: URL(string: "https://example.com/a.bin")!, filename: "a.bin", isEnabled: false)
    let itemB = DownloadItem(
        url: URL(string: "https://example.com/b.bin")!, filename: "b.bin", isEnabled: false)
    let packageAID = UUID()
    let packageBID = UUID()
    let packageCID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [itemA]))
    await engine.add(DownloadPackage(id: packageBID, name: "B", items: [itemB]))
    await engine.add(DownloadPackage(id: packageCID, name: "C"))

    await engine.moveItems([itemA.id, itemB.id], toPackage: packageCID)

    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first { $0.id == packageAID } == nil)
    #expect(snapshot.packages.first { $0.id == packageBID } == nil)
    #expect(
        snapshot.packages.first { $0.id == packageCID }?.items.map(\.id) == [itemA.id, itemB.id])
}

/// Same all-or-nothing refusal `moveItem` gives a single running item,
/// generalized: one running item anywhere in the batch refuses the whole
/// move rather than silently moving the rest.
@Test func moveItemsRefusesTheWholeBatchIfAnyMemberIsRunning() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let running = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let queued = DownloadItem(
        url: URL(string: "https://example.com/b.bin")!, filename: "b.bin", isEnabled: false)
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [running, queued]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))

    var spins = 0
    while await snapshotItem(running.id, in: engine)?.state != .running, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await engine.moveItems([running.id, queued.id], toPackage: packageBID)

    let snapshot = await engine.snapshot()
    #expect(
        snapshot.packages.first { $0.id == packageAID }?.items.map(\.id) == [running.id, queued.id])
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.isEmpty == true)

    await gate.open()
    try await engine.runUntilIdle()
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
