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

    // Any value strictly inside [0, 8000) works now that a single worker's
    // claim is always the whole file rather than half of it; 3000 keeps a
    // healthy margin from both ends.
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
    let secondOrigin = FakeOrigin(payload: payload, behavior: changed)
    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: secondOrigin,
        configuration: .test(workers: 1)
    )
    _ = try await resumed.start()

    let restarted = await resumed.completedRanges
    #expect(restarted.ranges == [ByteRange(start: 0, end: 4000)])
    #expect(try Data(contentsOf: destination) == payload)

    // Both origins serve the identical payload, so byte identity and the
    // coalesced completed-range assertions above would also pass under a
    // wrongly-*resumed* download that trusted the stale sidecar — they are
    // not proof the validator guard actually did anything. What can't be
    // faked: a wrong resume would only ask the second origin for the
    // remaining gap [1000, 4000); a correct restart-from-zero must genuinely
    // re-fetch the discarded prefix [0, 1000) too. Assert the second origin
    // actually saw requests covering the whole file, not just the tail.
    let secondOriginClaims = await secondOrigin.requestedRanges.filter { $0.length > 1 }
    var refetched = RangeSet()
    for claim in secondOriginClaims { refetched.insert(claim) }
    #expect(refetched.isComplete(total: 4000))
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

/// Hazard: `pause()` used to call `checkpoint()` unconditionally, including
/// after `start()` had already finished successfully. At that point `file`
/// is nil (the `.incomplete` file was already renamed to the destination),
/// but `completed` / `totalBytes` / `validator` still hold their *finished*
/// values, so the unconditional checkpoint would resurrect a `.sdmpart`
/// claiming the whole file is a complete in-progress download — with no
/// `.incomplete` file left to back that claim. A later `start()` against the
/// same destination would trust that sidecar, open a fresh all-zero
/// `.incomplete` file, consider it already complete, and try to finalize
/// garbage over (or in place of) the real file. This is a realistic UI race:
/// the user hits Pause just as the transfer completes.
@Test func pausingAfterSuccessfulCompletionDoesNotPoisonNextStart() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)

    // The race: pause() lands just after start() already finished.
    await task.pause()
    #expect(ResumeSidecar.load(from: ResumeSidecar.url(for: destination)) == nil)

    // A fresh task pointed at the same, already-finished destination has no
    // poisoned sidecar to trust, so it genuinely re-downloads the whole
    // file — and since `destination` still exists, `finalize()` correctly
    // refuses to overwrite it rather than silently replacing (or zero-
    // filling) it. That's a separate, pre-existing invariant of
    // `SparseFile.finalize()`, not something Task 10 changes; what matters
    // here is that the failure is loud and the original, correct file is
    // left completely untouched — never that a second `start()` on a live
    // destination should succeed.
    let again = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    await #expect(throws: (any Error).self) {
        _ = try await again.start()
    }
    #expect(try Data(contentsOf: destination) == payload)
}

/// The deleted-destination variant of the same race: if a stray `pause()`
/// after completion ever did resurrect a sidecar, and the user then deleted
/// the finished file, the next `start()` would have nothing to fail loudly
/// against — `finalize()` would succeed and silently install an all-zero
/// file. Verifying byte identity here closes that path.
@Test func pausingAfterCompletionThenDeletingDestinationStillRestartsCleanly() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()
    await task.pause()

    try FileManager.default.removeItem(at: destination)

    let again = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 4)
    )
    let result = try await again.start()
    #expect(try Data(contentsOf: result) == payload)
}

/// Isolates `prepare()`'s second line of defense from `pause()`'s guard:
/// even if a sidecar on disk claims the whole file is complete (matching
/// totalBytes and validator), if there is no `.incomplete` file backing that
/// claim, `prepare()` must not trust it — it must restart from zero rather
/// than mark the download complete over nonexistent bytes.
@Test func sidecarClaimingCompleteWithoutBackingFileRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    var full = RangeSet()
    full.insert(ByteRange(start: 0, end: 4000))
    let poisoned = ResumeSidecar(
        sourceURL: testSourceURL,
        totalBytes: 4000,
        validator: nil,
        completed: full
    )
    try poisoned.save(to: ResumeSidecar.url(for: destination))
    // No `.incomplete` file exists at all — the poisoned case.

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
}
