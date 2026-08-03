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

@Test func multipleWorkersActuallyRequestDisjointRanges() async throws {
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

    // Drop the 1-byte probe request from prepare().
    let claims = await origin.requestedRanges.filter { $0.length > 1 }
    #expect(claims.count > 1)

    var seen = RangeSet()
    for claim in claims {
        for other in seen.ranges {
            #expect(claim.start >= other.end || claim.end <= other.start)
        }
        seen.insert(claim)
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
