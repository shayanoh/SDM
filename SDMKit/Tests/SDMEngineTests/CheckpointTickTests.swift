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
