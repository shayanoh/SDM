import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Covers the time-based checkpoint trigger added alongside speed telemetry
/// (spec §4.3: checkpoint "every ~8 MB per worker or every 5 s, whichever
/// comes first"). `DownloadTask.record` already implements the byte trigger;
/// `checkpointTick()` is the engine's 1 Hz tick driving the wall-clock one,
/// so a slow or drip-feeding server can't go unbounded time without a
/// checkpoint.

@Test func checkpointTickWritesSidecarAfterFiveTicks() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    // dropAfterBytes: 0 fails the very first chunk, before any bytes are
    // recorded, so the byte trigger in `record` can never fire — only the
    // tick-driven trigger under test can produce a sidecar here.
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 0
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let sidecarURL = ResumeSidecar.url(for: destination)
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    for _ in 0..<4 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func checkpointTickResetsStalenessCounterAfterFiring() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 0
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let sidecarURL = ResumeSidecar.url(for: destination)
    for _ in 0..<5 {
        await task.checkpointTick()
    }
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
    try FileManager.default.removeItem(at: sidecarURL)

    // If the counter weren't reset when the checkpoint fired, it would
    // already be past threshold and the very next tick would re-fire.
    for _ in 0..<4 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func checkpointFromByteTriggerResetsTickCounterTooSoDoubleWriteCannotHappen() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    // minChunk 64 / checkpointInterval 128 (from `.test`); the drop at 300
    // bytes lands after the byte trigger has already fired at least once,
    // then kills the connection so `file` stays non-nil for the tick calls
    // below.
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 300
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let sidecarURL = ResumeSidecar.url(for: destination)
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
    try FileManager.default.removeItem(at: sidecarURL)

    for _ in 0..<4 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func checkpointTickDoesNotResurrectSidecarAfterSuccessfulCompletion() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()

    let sidecarURL = ResumeSidecar.url(for: destination)
    for _ in 0..<10 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func checkpointTickNeverWritesSidecarForNonResumableDownload() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    behavior.dropAfterBytes = 0
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let sidecarURL = ResumeSidecar.url(for: destination)
    for _ in 0..<10 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func checkpointTickIsANoOpBeforeStart() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )

    for _ in 0..<10 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: ResumeSidecar.url(for: destination).path))
}

// MARK: - Checkpoint durability and failure reporting

/// Makes the download's folder read-only at the moment the *body* request is
/// issued — i.e. after `prepare()` has already removed any stale sidecar and
/// opened the `.incomplete` file, but before the first checkpoint.
///
/// The open descriptor keeps working (`pwrite` doesn't consult directory
/// permissions), so bytes still land correctly at their absolute offsets; only
/// creating the new `.sdmpart` file fails. That is the real-world shape of the
/// bug — the download appears healthy while its resume state is silently
/// being lost.
private struct FolderLockingOrigin: HTTPTransport {
    let inner: FakeOrigin
    let folder: URL

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let response = try await inner.fetch(request)
        if let range = request.range, range.length > 1 {
            try? FileManager.default.setAttributes(
                [.posixPermissions: 0o500],
                ofItemAtPath: folder.path
            )
        }
        return response
    }
}

private func unlock(_ folder: URL) {
    try? FileManager.default.setAttributes(
        [.posixPermissions: 0o755],
        ofItemAtPath: folder.path
    )
}

/// A sidecar that cannot be written means a crash loses every byte of
/// progress with nothing anywhere saying so. `checkpoint()` used to swallow
/// the failure with `try?`.
@Test func aFailingCheckpointIsRecordedRatherThanSwallowed() async throws {
    let dir = try makeScratchDirectory()
    defer {
        unlock(dir)
        try? FileManager.default.removeItem(at: dir)
    }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 300
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FolderLockingOrigin(
            inner: FakeOrigin(payload: testPayload(4000), behavior: behavior),
            folder: dir
        ),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let reason = try #require(await task.lastCheckpointFailure)
    #expect(reason.contains("out.bin"))
    #expect(await task.checkpointFailureCount > 0)
    // The bytes themselves were unaffected — only the resume state was lost.
    #expect(await task.completedRanges.totalBytes == 300)
    #expect(!FileManager.default.fileExists(atPath: ResumeSidecar.url(for: destination).path))
}

/// The healthy path must leave no residue: a checkpoint that succeeds clears
/// any previously recorded failure, so a one-off hiccup doesn't stick to the
/// item forever.
@Test func aSuccessfulCheckpointReportsNoFailure() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 300
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    #expect(FileManager.default.fileExists(atPath: ResumeSidecar.url(for: destination).path))
    #expect(await task.lastCheckpointFailure == nil)
    #expect(await task.checkpointFailureCount == 0)
}

/// The engine carries the reason through to the snapshot the UI consumes, and
/// keeps it after the runner has retired — "this download's resume state could
/// not be written" is precisely the thing that must still be visible once the
/// download has stopped.
@Test func checkpointFailureReachesTheEngineSnapshot() async throws {
    let dir = try makeScratchDirectory()
    let folder = dir.appendingPathComponent("Batch")
    defer {
        unlock(folder)
        try? FileManager.default.removeItem(at: dir)
    }

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 300
    let engine = DownloadEngine(
        transport: FolderLockingOrigin(
            inner: FakeOrigin(payload: testPayload(4000), behavior: behavior),
            folder: folder
        ),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir,
            checkpointIntervalBytes: 128
        )
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [
            DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
        ]
    )
    await engine.add(package)
    try await engine.runUntilIdle()

    let item = try #require(await engine.snapshot().packages.first?.items.first)
    let reason = try #require(item.checkpointFailure)
    #expect(reason.contains("a.bin"))
}
