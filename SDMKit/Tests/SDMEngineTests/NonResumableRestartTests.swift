import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Verifies both halves of item 4's fix: `DownloadTask.prepare()` already
/// discarded on-disk bytes correctly for a non-resumable restart (see
/// `ResumeTests`'s validator-mismatch coverage for the equivalent byte-level
/// proof); what was missing was `DownloadEngine` itself not carrying the old
/// task's stale partial `completed` forward once the item lands somewhere
/// other than `.completed`.
///
/// Drives this through a genuine transient failure (a dropped connection
/// after partial bytes) rather than an explicit `stopItem()` call: both land
/// in `run()`'s `.none` retire-intent branch and reach `finish()` the same
/// way, but a synchronous, scripted `FakeOrigin` failure is deterministic,
/// whereas racing an explicit `stopItem()` against a worker's own in-flight
/// write is not — `retireRunnerNonBlocking`'s `pause()` call is intentionally
/// unstructured (see its doc comment) specifically so a user-facing Stop
/// never blocks, which means a test has no synchronization point to wait on
/// without reaching into engine internals.
@Test func aTransientFailureOnANonResumableDownloadDiscardsItsDisplayedProgress() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    behavior.chunkSize = 64
    behavior.dropAfterBytes = 1000
    let payload = testPayload(8000)
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload, behavior: behavior),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    // The dropped connection is transient, so the item is held in retry
    // backoff rather than promoted back to `.running` — `runUntilIdle()`
    // (which doesn't tick, so backoff never ages) settles on exactly that
    // state without racing a real retry attempt.
    try await engine.runUntilIdle()

    let final = await snapshotItem(item.id, in: engine)
    #expect(final?.state == .queued)
    #expect(final?.isResumable == false)
    #expect(final?.completed.totalBytes == 0)
}

/// Regression guard for the fix above: a non-resumable download that
/// genuinely finishes must keep its full `completed` range, not have it
/// zeroed just because `isResumable == false`.
@Test func nonResumableDownloadThatCompletesKeepsItsFullCompletedRange() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    let payload = testPayload(4000)
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload, behavior: behavior),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: testSourceURL, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    let snapshot = await snapshotItem(item.id, in: engine)
    #expect(snapshot?.state == .completed)
    #expect(snapshot?.isResumable == false)
    #expect(snapshot?.completed.totalBytes == Int64(payload.count))
}
