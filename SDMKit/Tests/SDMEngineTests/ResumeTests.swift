import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Runs a download to partial completion, then abandons the task object,
/// leaving `.incomplete` and `.sdmpart` on disk.
private func downloadPartially(
    payload: Data,
    destination: URL,
    stopAfter: Int
) async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = stopAfter
    behavior.chunkSize = 64
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 1)
    )
    _ = try? await task.start()
    await task.pause()
}

@Test func resumingFromSidecarProducesIdenticalFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 2000)
    #expect(ResumeSidecar.load(from: ResumeSidecar.url(for: destination)) != nil)

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 4)
    )
    let result = try await resumed.start()
    #expect(try Data(contentsOf: result) == payload)
}

@Test func resumeDoesNotRefetchAlreadyCompletedBytes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)

    // NOTE: 3000, not 4000. With this payload/config, `RangeSet.nextClaim`'s
    // halving policy hands the single worker a first claim of exactly
    // [0, 4000) — 4000 bytes. `dropAfterBytes: 4000` would then equal the
    // request length exactly, and `FakeOrigin`'s drop check
    // (`emitted + chunkLen > limit`) is strict, so it never fires: the
    // "partial" download would actually complete in full. 3000 is strictly
    // inside the first claim, so the interruption is real.
    try await downloadPartially(payload: payload, destination: destination, stopAfter: 3000)
    let carried = ResumeSidecar.load(from: ResumeSidecar.url(for: destination))!
    #expect(carried.completed.totalBytes > 0)

    let origin = FakeOrigin(payload: payload)
    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: origin,
        configuration: .test(workers: 1)
    )
    _ = try await resumed.start()

    let refetched = await origin.requestedRanges.filter { $0.length > 1 }
    let overlap = refetched.contains { claim in
        carried.completed.ranges.contains { $0.start < claim.end && claim.start < $0.end }
    }
    #expect(!overlap)
}

@Test func missingSidecarRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 1000)
    ResumeSidecar.remove(at: ResumeSidecar.url(for: destination))

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    #expect(try await Data(contentsOf: resumed.start()) == payload)
}

@Test func corruptSidecarRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 1000)
    try Data("garbage".utf8).write(to: ResumeSidecar.url(for: destination))

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    #expect(try await Data(contentsOf: resumed.start()) == payload)
}

@Test func changedValidatorDiscardsPartialAndRestarts() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    var behavior = FakeOrigin.Behavior()
    behavior.validator = "etag-1"
    behavior.dropAfterBytes = 1000
    behavior.chunkSize = 64
    let first = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 1)
    )
    _ = try? await first.start()
    await first.pause()

    var changed = FakeOrigin.Behavior()
    changed.validator = "etag-2"
    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: changed),
        configuration: .test(workers: 1)
    )
    _ = try await resumed.start()

    let restarted = await resumed.completedRanges
    #expect(restarted.ranges == [ByteRange(start: 0, end: 4000)])
    #expect(try Data(contentsOf: destination) == payload)
}

/// Hazard: `pause()` sets `file = nil` while workers may still be mid-write.
/// If `download(_:)` recorded bytes via optional chaining (`file?.write`),
/// a paused-mid-flight download could mark ranges complete in the sidecar
/// that were never actually written to disk — a Frankenstein file on
/// resume. This drives a real download to genuine concurrency (all workers
/// holding claims) before pausing, then verifies every byte the sidecar
/// claims as complete matches the source at that offset.
@Test func pausingMidFlightNeverRecordsUnwrittenBytes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(50_000)

    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 4, minChunk: 64)
    )

    async let outcome: Void = {
        _ = try? await task.start()
    }()

    // Wait for genuine mid-flight progress: at least one byte actually
    // written and recorded, with the pool still short of the full payload —
    // not just claims reserved (workers can reach `activeWorkerCount == 4`
    // before any of them has streamed a single byte, which would make this
    // test pass trivially without exercising the race at all).
    var spins = 0
    while await task.completedRanges.totalBytes == 0, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)
    #expect(await task.completedRanges.totalBytes < Int64(payload.count))

    await task.pause()
    _ = await outcome

    let sidecarURL = ResumeSidecar.url(for: destination)
    guard let sidecar = ResumeSidecar.load(from: sidecarURL) else {
        Issue.record("expected a sidecar to exist after pausing mid-flight")
        return
    }
    #expect(sidecar.completed.totalBytes > 0)
    #expect(sidecar.completed.totalBytes < Int64(payload.count))

    let incompleteURL = SparseFile.incompleteURL(for: destination)
    let onDisk = try Data(contentsOf: incompleteURL)
    for range in sidecar.completed.ranges {
        let recorded = onDisk.subdata(in: Int(range.start)..<Int(range.end))
        let expected = payload.subdata(in: Int(range.start)..<Int(range.end))
        #expect(recorded == expected)
    }
}
