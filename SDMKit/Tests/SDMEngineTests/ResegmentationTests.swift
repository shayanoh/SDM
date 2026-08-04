import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func eightWorkersProduceIdenticalOutput() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(20_000)
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 8)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
}

/// A worker's *requested* HTTP range can legitimately overlap another's once
/// claims can be split mid-flight: a claim stolen from a busy worker (see
/// `DownloadTask.claimNext`) shrinks that worker's live write boundary
/// without touching the wide `Range` header it already sent — the point of
/// stealing is to avoid re-issuing that request, not to keep requests
/// disjoint. What must stay disjoint is what actually lands on disk, which
/// `task.writeLog` (test-only) records directly from `record()`, independent
/// of `completedRanges` — `RangeSet.insert` would silently merge away an
/// overlap rather than reveal one.
@Test func multipleWorkersNeverWriteOverlappingBytes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let origin = FakeOrigin(payload: testPayload(20_000))
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: origin,
        configuration: .test(workers: 4)
    )
    _ = try await task.start()

    let written = await task.writeLog
    #expect(written.count > 1)

    var seen = RangeSet()
    for range in written {
        for other in seen.ranges {
            #expect(range.start >= other.end || range.end <= other.start)
        }
        seen.insert(range)
    }
}

@Test func loweringWorkerCountMidFlightStillCompletesFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(50_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 100, minChunk: 64)
    )

    async let result = task.start()
    await task.setWorkerCount(3)
    #expect(try await Data(contentsOf: result) == payload)
}

@Test func raisingWorkerCountMidFlightStillCompletesFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(50_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 2, minChunk: 64)
    )

    async let result = task.start()

    // Raise only once both initial workers hold claims, so this exercises a
    // genuine mid-flight raise rather than a target changed before the pool
    // starts. Bounded so a pool that never reaches 2 fails instead of hanging.
    var spins = 0
    while await task.activeWorkerCount < 2, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await task.setWorkerCount(16)
    #expect(try await Data(contentsOf: result) == payload)

    // Byte identity alone would pass even if the raise were a no-op; the peak
    // is what proves extra workers were actually spawned and ran.
    #expect(await task.peakWorkerCount > 2)
}

/// Spec §5.1: a fresh claim takes the whole free gap, so a single worker
/// downloading a file owns *all* of it — there is no free remainder left
/// for a raise to hand a second worker the ordinary way. The only source of
/// work for that second worker is stealing the unwritten tail of the first
/// worker's claim. This proves that happens, and — the part that matters
/// most — that the original worker's in-flight request is never restarted:
/// once its claim is stolen from, it keeps streaming the same request it
/// already had open, just stopping early where its shrunk claim ends. Once
/// both workers are down to small remainders they will legitimately keep
/// stealing from each other to finish the file (spec's "a freed-up worker
/// steals from a still-busy sibling" case), so this doesn't assert an exact
/// request count — only that the range starting at byte 0 was ever
/// requested once, proving that specific claim was never reissued.
@Test func raisingFromOneWorkerStealsHalfOfTheSoleInFlightClaimWithoutRestartingIt() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(50_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let origin = FakeOrigin(payload: payload, behavior: behavior)
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: origin,
        configuration: .test(workers: 1, minChunk: 64)
    )

    async let result = task.start()

    var spins = 0
    while await task.activeWorkerCount < 1, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await task.setWorkerCount(2)
    #expect(try await Data(contentsOf: result) == payload)
    #expect(await task.peakWorkerCount == 2)

    // At least the original claim plus the steal that split it; possibly
    // more, from the two workers repeatedly stealing from each other as
    // they run down to the last bytes.
    let claims = await origin.requestedRanges.filter { $0.length > 1 }
    #expect(claims.count >= 2)

    // The byte-0 claim was requested exactly once — a restart would show up
    // as a second, later request also starting at 0.
    #expect(claims.filter { $0.start == 0 }.count == 1)

    // What was actually written must still be strictly disjoint even though
    // the first worker's *request* spans the whole file.
    let written = await task.writeLog
    var seen = RangeSet()
    for range in written {
        for other in seen.ranges {
            #expect(range.start >= other.end || range.end <= other.start)
        }
        seen.insert(range)
    }
}

@Test(arguments: [1, 2, 3, 7, 13, 32])
func randomizedWorkerChurnPreservesByteIdentity(seed: Int) async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(40_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 128
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 4, minChunk: 64)
    )

    async let result = task.start()
    var generator = SeededGenerator(seed: UInt64(seed))
    for _ in 0..<12 {
        await task.setWorkerCount(Int.random(in: 1...24, using: &generator))
        await Task.yield()
    }
    #expect(try await Data(contentsOf: result) == payload)
}

/// Deterministic generator so a failing case is reproducible from its seed.
struct SeededGenerator: RandomNumberGenerator {
    private var state: UInt64
    init(seed: UInt64) { self.state = seed &* 6_364_136_223_846_793_005 &+ 1 }
    mutating func next() -> UInt64 {
        state ^= state << 13
        state ^= state >> 7
        state ^= state << 17
        return state
    }
}
