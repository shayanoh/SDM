import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Spec §6.4's failure handling, at the engine level.
///
/// The shape being guarded against: `run()` returned every transient failure
/// to `.queued`, `tick()` reconciled every second, the scheduler re-desired
/// the still-enabled incomplete item, and a fresh `DownloadTask` re-probed —
/// an unbounded request storm at one attempt per second per item, forever,
/// against any origin that 500s, truncates, or drops.

private func makeEngine(
    transport: any HTTPTransport,
    folder: URL,
    retryPolicy: RetryPolicy,
    segments: Int = 1
) -> DownloadEngine {
    DownloadEngine(
        transport: transport,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: segments,
            globalMaxConnections: 8,
            downloadFolder: folder
        ),
        retryPolicy: retryPolicy
    )
}

private func onePackage(_ name: String = "a.bin") -> DownloadPackage {
    DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/\(name)")!, filename: name)]
    )
}

/// Drives the heartbeat `count` times, draining whatever each tick starts.
private func pump(_ engine: DownloadEngine, ticks count: Int) async throws {
    for _ in 0..<count {
        await engine.tick()
        try await engine.runUntilIdle()
    }
}

private func stateOf(_ id: UUID, in engine: DownloadEngine) async -> ItemState? {
    await engine.snapshot().packages.flatMap(\.items).first { $0.id == id }?.state
}

// MARK: - Backoff and the attempt cap

/// A permanently-500ing origin must cost a bounded number of requests, not one
/// per tick. With `maxAttempts: 3` the item goes terminal after exactly three
/// attempts and issues nothing further no matter how long the engine runs.
@Test func transientFailuresBackOffAndBecomeTerminal() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 500
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let engine = makeEngine(
        transport: origin,
        folder: dir,
        retryPolicy: RetryPolicy(maxAttempts: 3, baseDelay: .seconds(2))
    )

    let package = onePackage()
    let itemID = package.items[0].id
    await engine.add(package)
    // `add()` reconciles, so attempt 1 is already in flight.
    try await engine.runUntilIdle()

    #expect(await stateOf(itemID, in: engine) == .queued)
    #expect(await origin.requestedRanges.count == 1)

    // Thirty heartbeats. Without backoff that is thirty requests.
    try await pump(engine, ticks: 30 * AppTiming.ticksPerSecond)

    let finalState = try #require(await stateOf(itemID, in: engine))
    guard case .failed(let reason) = finalState else {
        Issue.record("expected a terminal failure, got \(finalState)")
        return
    }
    #expect(reason.contains("3 attempts"))
    #expect(reason.contains("500"))
    #expect(await origin.requestedRanges.count == 3)
}

/// Backoff must delay an item, not wedge it: once the hold expires the item is
/// re-desired, and an origin that has recovered in the meantime completes it.
@Test func anItemHeldInBackoffResumesOnceTheOriginRecovers() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(2000)

    var behavior = FakeOrigin.Behavior()
    behavior.truncateAfterBytes = 500
    behavior.chunkSize = 100
    let origin = FakeOrigin(payload: payload, behavior: behavior)
    let engine = makeEngine(
        transport: origin,
        folder: dir,
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(2))
    )

    let package = onePackage()
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()
    #expect(await stateOf(itemID, in: engine) == .queued)

    // The item is in backoff right now: ticking once must not re-attempt it.
    let requestsAfterFirstFailure = await origin.requestedRanges.count
    await engine.tick()
    try await engine.runUntilIdle()
    #expect(await origin.requestedRanges.count == requestsAfterFirstFailure)

    await origin.setBehavior(FakeOrigin.Behavior())
    try await pump(engine, ticks: 10 * AppTiming.ticksPerSecond)

    #expect(await stateOf(itemID, in: engine) == .completed)
    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    #expect(try Data(contentsOf: destination) == payload)
}

private func itemSnapshot(_ id: UUID, in engine: DownloadEngine) async -> ItemSnapshot? {
    await engine.snapshot().packages.flatMap(\.items).first { $0.id == id }
}

/// A transient failure that returns the item to `.queued` must not look
/// identical to a fresh queue: the snapshot carries the last error, the
/// consecutive-attempt count, and how long the backoff hold has left — and
/// all three clear once the origin recovers and the item completes.
@Test func aTransientFailureSurfacesTheLastErrorAndRetryHoldInTheSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(2000)

    var behavior = FakeOrigin.Behavior()
    behavior.truncateAfterBytes = 500
    behavior.chunkSize = 100
    let origin = FakeOrigin(payload: payload, behavior: behavior)
    let engine = makeEngine(
        transport: origin,
        folder: dir,
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(2))
    )

    let package = onePackage()
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    let held = try #require(await itemSnapshot(itemID, in: engine))
    #expect(held.state == .queued)
    #expect(held.isRetrying)
    #expect(held.failedAttemptCount == 1)
    #expect(held.lastFailureReason != nil)
    #expect((held.retryHoldSeconds ?? 0) > 0)

    await origin.setBehavior(FakeOrigin.Behavior())
    try await pump(engine, ticks: 10 * AppTiming.ticksPerSecond)

    let done = try #require(await itemSnapshot(itemID, in: engine))
    #expect(done.state == .completed)
    #expect(!done.isRetrying)
    #expect(done.lastFailureReason == nil)
    #expect(done.retryHoldSeconds == nil)
    #expect(done.failedAttemptCount == nil)
}

// MARK: - Permanent local failures

/// `DownloadTask.finalize()` refuses to overwrite an existing destination, and
/// that refusal is right. The engine used to classify the resulting `NSError`
/// as transient, so the item returned to `queued`, resumed to "complete"
/// instantly from its sidecar on the next tick, and failed at the same rename
/// again — forever. Adding the same URL twice reached this immediately.
@Test func anExistingDestinationFailsPermanentlyInsteadOfLooping() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let folder = dir.appendingPathComponent("Batch")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let existing = Data("do not clobber me".utf8)
    try existing.write(to: folder.appendingPathComponent("a.bin"))

    let origin = FakeOrigin(payload: testPayload(1000))
    let engine = makeEngine(
        transport: origin,
        folder: dir,
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(1))
    )

    let package = onePackage()
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    let state = try #require(await stateOf(itemID, in: engine))
    guard case .failed(let reason) = state else {
        Issue.record("expected a terminal failure, got \(state)")
        return
    }
    #expect(reason.contains("already exists"))

    let requestsAtFailure = await origin.requestedRanges.count
    try await pump(engine, ticks: 20 * AppTiming.ticksPerSecond)
    // Terminal means terminal: not one further request, and the user's file
    // is untouched.
    #expect(await origin.requestedRanges.count == requestsAtFailure)
    #expect(try Data(contentsOf: folder.appendingPathComponent("a.bin")) == existing)
}

/// `context(for:)` used to create the package folder with `try?`. When that
/// failed, `SparseFile` failed to open, the failure classified transient, and
/// the item entered the same hot loop with nothing anywhere saying why. A file
/// sitting where the folder should be is the cheapest way to reproduce it.
@Test func anUncreatableDestinationFolderIsReportedNotRetried() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    // "Batch" is a regular file, so createDirectory cannot succeed.
    try Data("blocked".utf8).write(to: dir.appendingPathComponent("Batch"))

    let origin = FakeOrigin(payload: testPayload(1000))
    let engine = makeEngine(
        transport: origin,
        folder: dir,
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(1))
    )

    let package = onePackage()
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    let state = try #require(await stateOf(itemID, in: engine))
    guard case .failed(let reason) = state else {
        Issue.record("expected a terminal failure, got \(state)")
        return
    }
    #expect(reason.contains("Could not create the folder"))
    #expect(reason.contains("Batch"))
    // Never even reached the network, and never retries.
    #expect(await origin.requestedRanges.isEmpty)
    try await pump(engine, ticks: 10 * AppTiming.ticksPerSecond)
    #expect(await origin.requestedRanges.isEmpty)
}

// Preemption must not be charged to the retry budget — see
// `preemptionIsNotChargedToTheRetryBudget` in `DownloadEngineTests.swift`,
// which has the gated transport needed to park a download mid-body and
// preempt it deterministically.
