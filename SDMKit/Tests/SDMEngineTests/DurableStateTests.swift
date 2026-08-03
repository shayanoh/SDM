import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Spec §4.2: durable state is written "debounced (~2 s after the last
/// change), plus on quit".
///
/// `JSONStateStore.save()` only stores into `pending` in memory; `flush()` is
/// the sole writer of `state.json`. Its only caller anywhere used to be
/// `DownloadEngine.shutdown()`, which runs only after the heartbeat's
/// `while !Task.isCancelled` loop exits — i.e. only if the cancelled `.task`
/// continuation gets scheduled before the process dies. On ⌘Q it does not. So
/// in the ordinary case *nothing was ever written*: the `.incomplete` file and
/// its `.sdmpart` survived with no record of the item owning them, and the
/// next launch restored an empty list.

private func stoppedPackage(_ name: String = "a.bin") -> DownloadPackage {
    var item = DownloadItem(
        url: URL(string: "https://example.com/\(name)")!,
        filename: name
    )
    item.isEnabled = false
    return DownloadPackage(name: "Batch", items: [item])
}

@Test func heartbeatWritesDurableStateWithoutAnyShutdown() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let stateURL = dir.appendingPathComponent("state.json")

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(100)),
        stateStore: JSONStateStore(fileURL: stateURL),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir,
            persistDebounceTicks: 2
        )
    )

    await engine.add(stoppedPackage())
    // A save alone is in-memory only; nothing has been written yet.
    #expect(!FileManager.default.fileExists(atPath: stateURL.path))

    await engine.tick()
    #expect(!FileManager.default.fileExists(atPath: stateURL.path))

    // Second tick crosses the debounce window. No shutdown() anywhere.
    await engine.tick()
    #expect(FileManager.default.fileExists(atPath: stateURL.path))

    let reread = await JSONStateStore(fileURL: stateURL).load()
    #expect(reread.packages.flatMap(\.items).map(\.filename) == ["a.bin"])
}

/// The debounce must be cheap when idle: a tick with nothing dirty must not
/// re-encode and re-write the snapshot every second forever.
@Test func aQuiescentEngineStopsWritingDurableState() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let stateURL = dir.appendingPathComponent("state.json")

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(100)),
        stateStore: JSONStateStore(fileURL: stateURL),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir,
            persistDebounceTicks: 2
        )
    )

    await engine.add(stoppedPackage())
    await engine.tick()
    await engine.tick()
    #expect(FileManager.default.fileExists(atPath: stateURL.path))

    try FileManager.default.removeItem(at: stateURL)
    for _ in 0..<10 { await engine.tick() }
    #expect(!FileManager.default.fileExists(atPath: stateURL.path))
}

// MARK: - The real end-to-end restart

/// The completion criterion, tested for real: "Quitting mid-download and
/// relaunching resumes rather than restarting."
///
/// Nothing here is simulated. A `JSONStateStore` over an actual file, a real
/// `.incomplete` file and `.sdmpart` sidecar in a real download folder, a
/// download taken partway by an origin that truncates, a `shutdown()`, then a
/// *completely fresh* engine constructed over the same store and folder. The
/// existing `restoreRepopulatesPackagesWithProgressIntact` deliberately used
/// `InMemoryStateStore` with no backing files, which is why it could not have
/// caught the missing flush.
///
/// The second engine gets its own `FakeOrigin`, so its `requestedRanges` is a
/// clean record of what the resumed run actually asked for — which is how "did
/// not restart" is proved rather than asserted.
@Test func quittingMidDownloadAndRelaunchingResumesRatherThanRestarting() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let stateURL = dir.appendingPathComponent("state.json")
    let payload = testPayload(4000)
    let prefix: Int64 = 2000

    let package = DownloadPackage(
        name: "Batch",
        items: [
            DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
        ]
    )
    let itemID = package.items[0].id
    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")

    // --- Launch 1: gets 2000 of 4000 bytes, then the origin truncates. ---
    var behavior = FakeOrigin.Behavior()
    behavior.truncateAfterBytes = Int(prefix)
    behavior.chunkSize = 250
    let firstEngine = DownloadEngine(
        transport: FakeOrigin(payload: payload, behavior: behavior),
        stateStore: JSONStateStore(fileURL: stateURL),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir,
            checkpointIntervalBytes: 500
        )
    )
    await firstEngine.add(package)
    try await firstEngine.runUntilIdle()

    let midway = try #require(
        await firstEngine.snapshot().packages.flatMap(\.items).first { $0.id == itemID }
    )
    #expect(midway.completed.totalBytes == prefix)

    // Quit.
    await firstEngine.shutdown()

    // The three things a relaunch depends on all actually exist on disk.
    #expect(FileManager.default.fileExists(atPath: stateURL.path))
    #expect(
        FileManager.default.fileExists(atPath: SparseFile.incompleteURL(for: destination).path)
    )
    let sidecar = try #require(ResumeSidecar.load(from: ResumeSidecar.url(for: destination)))
    #expect(sidecar.completed.totalBytes == prefix)
    #expect(!FileManager.default.fileExists(atPath: destination.path))

    // What actually landed in state.json — the item, and its progress.
    let persisted = await JSONStateStore(fileURL: stateURL).load()
    let persistedItem = try #require(persisted.packages.flatMap(\.items).first)
    #expect(persistedItem.id == itemID)
    #expect(persistedItem.filename == "a.bin")
    #expect(persistedItem.completed.totalBytes == prefix)
    #expect(persistedItem.totalBytes == 4000)

    // --- Launch 2: a brand new engine over the same store and folder. ---
    let secondOrigin = FakeOrigin(payload: payload)
    let secondEngine = DownloadEngine(
        transport: secondOrigin,
        stateStore: JSONStateStore(fileURL: stateURL),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir,
            checkpointIntervalBytes: 500
        )
    )
    await secondEngine.restore()

    // The item came back from the JSON file. Its progress is not re-asserted
    // here: `restore()` reconciles, so by now a runner exists and `snapshot()`
    // reports the *task's* set, which is empty until `prepare()` has loaded
    // the sidecar. The persisted figure was checked above; what the resumed
    // run does with it is proved below, by what it asks the origin for.
    let restored = try #require(
        await secondEngine.snapshot().packages.flatMap(\.items).first { $0.id == itemID }
    )
    #expect(restored.filename == "a.bin")

    try await secondEngine.runUntilIdle()

    // Byte identity.
    #expect(try Data(contentsOf: destination) == payload)
    #expect(
        await secondEngine.snapshot().packages.flatMap(\.items).first { $0.id == itemID }?.state
            == .completed
    )

    // Resumed, not restarted: apart from `prepare()`'s one-byte probe, the
    // second run never asked for a byte it already had.
    let bodyRequests = await secondOrigin.requestedRanges.filter { $0.length > 1 }
    #expect(!bodyRequests.isEmpty)
    for request in bodyRequests {
        #expect(request.start >= prefix)
    }
}
