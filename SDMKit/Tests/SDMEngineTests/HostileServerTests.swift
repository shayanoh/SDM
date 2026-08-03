import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func serverIgnoringRangesForcesSingleWorker() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(5000)
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 16)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
    #expect(await task.supportsRanges == false)
    // The clamp itself: without it, 16 workers would spawn, one would
    // reserve the whole gap, and the other 15 would simply get `nil` from
    // `claimNext` and return — byte identity and `supportsRanges` alone
    // don't prove the pool was ever actually held to 1.
    #expect(await task.peakWorkerCount == 1)
}

@Test func serverOverstatingSizeFailsRatherThanTruncating() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.reportedSizeOverride = 9999
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(1000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    // `reportedSizeOverride` makes the probe claim 9999 bytes while the
    // actual payload is 1000. The single worker's first claim (half of the
    // reported 9999) asks for 0..4999; FakeOrigin clips the slice to the
    // real payload and ends the stream cleanly at byte 1000 — a clean short
    // read, which `download(_:)` already reports precisely as
    // `.truncatedResponse`. That is a more specific, and correct, failure
    // than the generic `.incompleteAfterWorkersFinished` the original brief
    // predates (written before Task 9 introduced `.truncatedResponse`); see
    // the task report for details.
    await #expect(
        throws: DownloadError.truncatedResponse(expected: 4999, received: 1000)
    ) {
        _ = try await task.start()
    }
}

@Test func midBodyDropLeavesRecoverablePartialProgress() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 500
    behavior.chunkSize = 64
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    _ = try? await task.start()
    await task.pause()

    let sidecar = ResumeSidecar.load(from: ResumeSidecar.url(for: destination))
    #expect(sidecar != nil)
    #expect(sidecar!.completed.totalBytes >= 448)
}

@Test func partialFileIsNeverRenamedIntoPlace() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 200
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    _ = try? await task.start()
    #expect(!FileManager.default.fileExists(atPath: destination.path))
}

/// Regression test for Finding 1 (round 1 review): a `Range`-ignoring
/// origin cannot be resumed. If an interrupted run against such an origin
/// were allowed to checkpoint a sidecar, a second run against the same
/// destination would trust that sidecar's non-zero `completed` set, hand
/// `claimNext` a non-zero-start claim, and then splice a clean origin's
/// from-byte-0 body into the middle of the file — a silent, unflagged
/// corruption that still finalizes successfully. Confirmed this test fails
/// (byte-identity mismatch) without the `prepare()`/`checkpoint()` fix; see
/// the task report.
@Test func ignoresRangesResumeNeverProducesCorruptFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(5000)

    var dropBehavior = FakeOrigin.Behavior()
    dropBehavior.ignoresRanges = true
    dropBehavior.dropAfterBytes = 500
    dropBehavior.chunkSize = 64
    let firstTask = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: dropBehavior),
        configuration: .test(workers: 1)
    )
    _ = try? await firstTask.start()
    await firstTask.pause()

    var cleanBehavior = FakeOrigin.Behavior()
    cleanBehavior.ignoresRanges = true
    let secondTask = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: cleanBehavior),
        configuration: .test(workers: 1)
    )
    let result = try await secondTask.start()
    #expect(try Data(contentsOf: result) == payload)
}
