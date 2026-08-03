import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func makeEngine(
    payload: Data,
    folder: URL,
    maxConcurrent: Int = 2,
    segments: Int = 2
) -> DownloadEngine {
    DownloadEngine(
        transport: FakeOrigin(payload: payload),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: maxConcurrent,
            segmentsPerItem: segments,
            globalMaxConnections: 32,
            downloadFolder: folder
        )
    )
}

private func packageOf(_ names: [String]) -> DownloadPackage {
    DownloadPackage(
        name: "Batch",
        items: names.enumerated().map { index, name in
            DownloadItem(
                url: URL(string: "https://example.com/\(name)")!,
                filename: name,
                position: index
            )
        }
    )
}

@Test func engineDownloadsAllItemsIntoAPackageFolder() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(3000)

    let engine = makeEngine(payload: payload, folder: dir)
    await engine.add(packageOf(["a.bin", "b.bin"]))
    try await engine.runUntilIdle()

    let packageFolder = dir.appendingPathComponent("Batch")
    #expect(try Data(contentsOf: packageFolder.appendingPathComponent("a.bin")) == payload)
    #expect(try Data(contentsOf: packageFolder.appendingPathComponent("b.bin")) == payload)
}

@Test func disabledItemsAreNotDownloaded() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(1000), folder: dir)
    var package = packageOf(["a.bin", "b.bin"])
    package.items[1].isEnabled = false
    await engine.add(package)
    try await engine.runUntilIdle()

    let folder = dir.appendingPathComponent("Batch")
    #expect(FileManager.default.fileExists(atPath: folder.appendingPathComponent("a.bin").path))
    #expect(!FileManager.default.fileExists(atPath: folder.appendingPathComponent("b.bin").path))
}

@Test func concurrencyLimitIsRespected() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(
        payload: testPayload(20_000),
        folder: dir,
        maxConcurrent: 2
    )
    await engine.add(packageOf(["a.bin", "b.bin", "c.bin", "d.bin"]))
    await engine.tick()

    let running = await engine.snapshot().packages
        .flatMap(\.items)
        .filter { $0.state == .running }
    #expect(running.count <= 2)

    try await engine.runUntilIdle()
}

@Test func snapshotAggregatesSpeedAcrossItems() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(4000), folder: dir)
    await engine.add(packageOf(["a.bin", "b.bin"]))
    try await engine.runUntilIdle()
    await engine.tick()

    let snapshot = await engine.snapshot()
    let itemTotal = snapshot.packages
        .flatMap(\.items)
        .reduce(0.0) { $0 + $1.bytesPerSecond }
    #expect(abs(snapshot.globalBytesPerSecond - itemTotal) < 0.001)
    // The aggregate must be a real number, not two zeroes agreeing with
    // each other: 8000 bytes across two items landed in one window.
    #expect(snapshot.globalBytesPerSecond > 0)
}

@Test func snapshotReportsSegmentedProgressRanges() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(2000), folder: dir)
    await engine.add(packageOf(["a.bin"]))
    try await engine.runUntilIdle()

    let item = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(item.completed.ranges == [ByteRange(start: 0, end: 2000)])
    #expect(item.state == .completed)
    #expect(item.totalBytes == 2000)
}

@Test func shutdownFlushesStateToTheStore() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let store = InMemoryStateStore()
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )
    await engine.add(packageOf(["a.bin"]))
    await engine.shutdown()

    #expect(await store.load().packages.map(\.name) == ["Batch"])
}

// MARK: - A transport that can be suspended at a chosen point

private enum GatePhase: Sendable {
    /// Block inside `fetch` for the one-byte probe, i.e. inside `prepare()`.
    case probe
    /// Block halfway through a body stream, i.e. inside `download(_:)`.
    case body
}

/// Lets a test park a download at an exact point and release it later,
/// without sleeping on a real clock.
private actor Gate {
    private var isOpen = false
    private var hasArrived = false
    private var openWaiters: [CheckedContinuation<Void, Never>] = []
    private var arrivalWaiters: [CheckedContinuation<Void, Never>] = []

    func signalArrival() {
        hasArrived = true
        let waiters = arrivalWaiters
        arrivalWaiters.removeAll()
        for waiter in waiters { waiter.resume() }
    }

    /// Returns once a download has reached the gate.
    func waitForArrival() async {
        if hasArrived { return }
        await withCheckedContinuation { arrivalWaiters.append($0) }
    }

    func open() {
        isOpen = true
        let waiters = openWaiters
        openWaiters.removeAll()
        for waiter in waiters { waiter.resume() }
    }

    func waitUntilOpen() async {
        if isOpen { return }
        await withCheckedContinuation { openWaiters.append($0) }
    }
}

private struct GatedOrigin: HTTPTransport {
    let payload: Data
    let gate: Gate
    let phase: GatePhase

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let total = Int64(payload.count)
        let range = request.range ?? ByteRange(start: 0, end: total)
        let isProbe = range.start == 0 && range.end == 1

        if isProbe {
            if phase == .probe {
                await gate.signalArrival()
                await gate.waitUntilOpen()
            }
            return RangeResponse(
                statusCode: 206,
                totalSize: total,
                acceptsRanges: true,
                validator: "etag-gated",
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let lower = Int(Swift.min(range.start, total))
        let upper = Int(Swift.min(range.end, total))
        let slice = payload.subdata(in: lower..<upper)
        let gate = self.gate
        let phase = self.phase
        let body = AsyncThrowingStream<Data, any Error> { continuation in
            guard phase == .body else {
                continuation.yield(slice)
                continuation.finish()
                return
            }
            Task {
                let split = slice.count / 2
                if split > 0 { continuation.yield(Data(slice.prefix(split))) }
                await gate.signalArrival()
                await gate.waitUntilOpen()
                continuation.yield(Data(slice.dropFirst(split)))
                continuation.finish()
            }
        }

        return RangeResponse(
            statusCode: 206,
            totalSize: total,
            acceptsRanges: true,
            validator: "etag-gated",
            body: body
        )
    }
}

private func makeGatedEngine(
    payload: Data,
    folder: URL,
    gate: Gate,
    phase: GatePhase
) -> DownloadEngine {
    DownloadEngine(
        transport: GatedOrigin(payload: payload, gate: gate, phase: phase),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: folder
        )
    )
}

/// Spec §4.3 requires the sidecar be checkpointed "every ~8 MB per worker or
/// every 5 s, whichever comes first". `DownloadTask.checkpointTick()` is the
/// wall-clock half; if the engine's `tick()` never calls it, the trigger is
/// dead code in production and the spec is unmet. This parks a download
/// mid-body — far below the 8 MB byte trigger — and shows that ticking alone
/// produces the sidecar, on the tick the staleness threshold is reached and
/// not before.
@Test func tickingCheckpointsRunningDownloads() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let gate = Gate()
    let engine = makeGatedEngine(
        payload: testPayload(4000),
        folder: dir,
        gate: gate,
        phase: .body
    )
    await engine.add(packageOf(["a.bin"]))
    await gate.waitForArrival()

    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    let sidecarURL = ResumeSidecar.url(for: destination)

    for _ in 0..<4 { await engine.tick() }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await engine.tick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
    let sidecar = try #require(ResumeSidecar.load(from: sidecarURL))
    #expect(sidecar.completed.totalBytes > 0)

    await gate.open()
    try await engine.runUntilIdle()
    #expect(try Data(contentsOf: destination) == testPayload(4000))
}

/// `DownloadTask.start()` awaits `transport.fetch` inside `prepare()`. A
/// `pause()` arriving at that suspension point used to be silently discarded:
/// it set `targetWorkerCount = 0`, but `runWorkers()` then reset it to the
/// configured count and the download proceeded anyway. The engine preempts by
/// pausing, so it reaches this window for real. Disabling an item is the
/// deterministic way to drive it — the item is then ineligible, so nothing
/// restarts it and the assertions cannot be masked by a later attempt.
@Test func pausingDuringPrepareActuallyStopsTheDownload() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let gate = Gate()
    let engine = makeGatedEngine(
        payload: testPayload(4000),
        folder: dir,
        gate: gate,
        phase: .probe
    )
    let package = packageOf(["a.bin"])
    let itemID = package.items[0].id
    await engine.add(package)

    // prepare() is now suspended inside transport.fetch.
    await gate.waitForArrival()
    await engine.setEnabled(false, for: itemID)
    await gate.open()
    try await engine.runUntilIdle()

    let item = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(item.state == .queued)
    #expect(item.completed.totalBytes == 0)
    #expect(
        !FileManager.default.fileExists(
            atPath: dir.appendingPathComponent("Batch")
                .appendingPathComponent("a.bin").path
        )
    )
}

private func snapshotItem(_ id: UUID, in engine: DownloadEngine) async -> ItemSnapshot? {
    await engine.snapshot().packages.flatMap(\.items).first { $0.id == id }
}

/// A preempted item must come back as `queued`, keep the bytes it already had
/// (so the later attempt resumes rather than restarts), and must not be
/// reported as `failed`. Preemption here is by priority: `b.bin` outranks the
/// running `a.bin` with only one slot available. `a.bin` is marked resumable
/// because spec §6 pass 1 makes running *non*-resumable items unpreemptible.
@Test func preemptedItemReturnsToQueuedAndResumesLater() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    let gate = Gate()
    let engine = makeGatedEngine(payload: payload, folder: dir, gate: gate, phase: .body)

    var first = packageOf(["a.bin"])
    first.items[0].isResumable = true
    let itemID = first.items[0].id
    await engine.add(first)

    // Wait for genuine mid-flight progress — a reserved claim alone would let
    // the progress-preservation assertion below pass trivially.
    await gate.waitForArrival()
    var spins = 0
    while await snapshotItem(itemID, in: engine)?.completed.totalBytes == 0, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    let second = DownloadPackage(
        name: "Urgent",
        items: [
            DownloadItem(
                url: URL(string: "https://example.com/b.bin")!,
                filename: "b.bin",
                position: 0
            )
        ],
        priority: .highest,
        position: 1
    )
    await engine.add(second)

    let midway = try #require(await snapshotItem(itemID, in: engine))
    #expect(midway.state == .queued)
    #expect(midway.completed.totalBytes > 0)

    await gate.open()
    try await engine.runUntilIdle()

    let batch = dir.appendingPathComponent("Batch")
    let urgent = dir.appendingPathComponent("Urgent")
    #expect(try Data(contentsOf: batch.appendingPathComponent("a.bin")) == payload)
    #expect(try Data(contentsOf: urgent.appendingPathComponent("b.bin")) == payload)
}
