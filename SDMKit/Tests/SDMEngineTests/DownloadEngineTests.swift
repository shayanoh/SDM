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
    /// Serve the whole body regardless of the requested range, and advertise
    /// no `Range` support — the shape that makes a download non-resumable.
    var ignoresRanges: Bool = false

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
                statusCode: ignoresRanges ? 200 : 206,
                totalSize: total,
                acceptsRanges: !ignoresRanges,
                validator: "etag-gated",
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let lower = ignoresRanges ? 0 : Int(Swift.min(range.start, total))
        let upper = ignoresRanges ? payload.count : Int(Swift.min(range.end, total))
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
            statusCode: ignoresRanges ? 200 : 206,
            totalSize: total,
            acceptsRanges: !ignoresRanges,
            validator: "etag-gated",
            body: body
        )
    }
}

private func makeGatedEngine(
    payload: Data,
    folder: URL,
    gate: Gate,
    phase: GatePhase,
    ignoresRanges: Bool = false,
    retryPolicy: RetryPolicy = RetryPolicy()
) -> DownloadEngine {
    DownloadEngine(
        transport: GatedOrigin(
            payload: payload,
            gate: gate,
            phase: phase,
            ignoresRanges: ignoresRanges
        ),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: folder
        ),
        retryPolicy: retryPolicy
    )
}

/// Preemption unwinds a runner by cancelling its job and pausing its task, so
/// `start()` throws — exactly the shape a real failure has. Charging it to the
/// retry budget would let a perfectly healthy download be marked `.failed` for
/// having been outranked too often, and would hold it in backoff instead of
/// letting it resume the moment a slot frees up.
///
/// `maxAttempts: 1` makes any charge at all immediately terminal, so the
/// assertion cannot pass by accident.
@Test func preemptionIsNotChargedToTheRetryBudget() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    let gate = Gate()
    let engine = makeGatedEngine(
        payload: payload,
        folder: dir,
        gate: gate,
        phase: .body,
        retryPolicy: RetryPolicy(maxAttempts: 1, baseDelay: .seconds(30))
    )

    let package = packageOf(["a.bin"])
    let itemID = package.items[0].id
    await engine.add(package)

    await gate.waitForArrival()
    // Disabling retires the in-flight runner through the preemption path.
    await engine.setEnabled(false, for: itemID)
    await gate.open()
    try await engine.runUntilIdle()

    #expect(await snapshotItem(itemID, in: engine)?.state == .queued)

    // A 30 s backoff would keep it out of the desired set for 30 ticks; a
    // charged attempt against maxAttempts 1 would have made it terminal. It
    // must simply start again on the very next reconcile.
    await engine.setEnabled(true, for: itemID)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
    #expect(
        try Data(
            contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
        ) == payload
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
    let package = packageOf(["a.bin"])
    let itemID = package.items[0].id
    await engine.add(package)

    // `waitForArrival` only proves the *producer* reached the gate. Spin until
    // the worker has actually consumed and recorded the first slice, so the
    // sidecar assertions below cannot depend on scheduler luck.
    await gate.waitForArrival()
    var spins = 0
    while await snapshotItem(itemID, in: engine)?.completed.totalBytes == 0, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

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

/// Preemption in production shape: nothing here hand-sets `isResumable`. The
/// items are built exactly as a grabber would hand them over — flag `nil`,
/// meaning "not probed yet" — and it is the engine that learns the origin
/// honors `Range` and writes `true` onto the item, which is what lets spec §6
/// pass 1 fall through and the higher-priority `b.bin` take the only slot.
///
/// A preempted item must come back as `queued` and keep the bytes it already
/// had, so the later attempt resumes rather than restarts — proved by the
/// byte-identity assertion after the drain.
@Test func preemptedItemReturnsToQueuedAndResumesLater() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    let gate = Gate()
    let engine = makeGatedEngine(payload: payload, folder: dir, gate: gate, phase: .body)

    let first = packageOf(["a.bin"])
    let itemID = first.items[0].id
    #expect(first.items[0].isResumable == nil)
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
    // The engine, not the test, discovered this.
    #expect(midway.isResumable == true)

    await gate.open()
    try await engine.runUntilIdle()

    let batch = dir.appendingPathComponent("Batch")
    let urgent = dir.appendingPathComponent("Urgent")
    #expect(try Data(contentsOf: batch.appendingPathComponent("a.bin")) == payload)
    #expect(try Data(contentsOf: urgent.appendingPathComponent("b.bin")) == payload)

    // Telemetry must account for the payload exactly once across the whole
    // preempt/resume cycle. Seeding the sampler baseline from the item's
    // resumed progress is what prevents the pre-preemption span being counted
    // a second time by the second attempt; seeding it at zero would report
    // 6000 bytes moved for a 4000-byte file.
    await engine.tick()
    let finished = try #require(await snapshotItem(itemID, in: engine))
    #expect(finished.speedHistory.reduce(0, +) == Double(payload.count))
}

/// The other half of the ruling, and the transition that actually matters:
/// `nil → false`. Against an origin that ignores `Range`, the download cannot
/// be resumed, so preempting it would throw away every byte it has. The engine
/// must learn that from the probe and write it onto the item, at which point
/// spec §6.3 pass 1 gives it an unconditional claim on its slot and the
/// higher-priority `b.bin` has to wait — the exact opposite outcome to
/// `preemptedItemReturnsToQueuedAndResumesLater`, from the same setup and with
/// the flag again never touched by the test.
@Test func nonResumableRunningItemIsNotPreemptedOnceProbed() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    let gate = Gate()
    let engine = makeGatedEngine(
        payload: payload,
        folder: dir,
        gate: gate,
        phase: .body,
        ignoresRanges: true
    )

    let first = packageOf(["a.bin"])
    let itemID = first.items[0].id
    #expect(first.items[0].isResumable == nil)
    await engine.add(first)

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
    let urgentID = second.items[0].id
    await engine.add(second)

    let held = try #require(await snapshotItem(itemID, in: engine))
    #expect(held.isResumable == false)
    #expect(held.state == .running)
    let waiting = try #require(await snapshotItem(urgentID, in: engine))
    #expect(waiting.state == .queued)

    await gate.open()
    try await engine.runUntilIdle()

    #expect(
        try Data(contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin"))
            == payload
    )
    #expect(
        try Data(contentsOf: dir.appendingPathComponent("Urgent").appendingPathComponent("b.bin"))
            == payload
    )
}

/// Isolates the scheduler rule itself. `refreshResumability()` normally lands
/// before any scheduling decision, so both `isResumable == false` and the
/// naive `!= true` produce the same answer once a probe has returned. They
/// differ only in the window this test occupies: a runner exists but its probe
/// has not answered, so the flag is genuinely `nil`. Treating unknown as
/// non-resumable would hand that item an unconditional claim on the only slot
/// and make it unpreemptible — for a download that has not transferred a
/// single byte and has nothing to lose.
@Test func unprobedRunningItemIsStillPreemptible() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    let gate = Gate()
    let engine = makeGatedEngine(payload: payload, folder: dir, gate: gate, phase: .probe)

    let first = packageOf(["a.bin"])
    let itemID = first.items[0].id
    await engine.add(first)

    // Parked inside prepare(); the probe has not answered, so nothing knows
    // yet whether this download is resumable.
    await gate.waitForArrival()

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
    let urgentID = second.items[0].id
    await engine.add(second)

    let unprobed = try #require(await snapshotItem(itemID, in: engine))
    #expect(unprobed.isResumable == nil)
    #expect(unprobed.state == .queued)
    let urgent = try #require(await snapshotItem(urgentID, in: engine))
    #expect(urgent.state == .running)

    await gate.open()
    try await engine.runUntilIdle()

    #expect(
        try Data(contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin"))
            == payload
    )
    #expect(
        try Data(contentsOf: dir.appendingPathComponent("Urgent").appendingPathComponent("b.bin"))
            == payload
    )
}

// MARK: - Restore

/// Engine-level mirror of the `DownloadTask` resume tests: a package graph
/// with partial progress and a `.running` item is placed directly in the
/// store — the shape a hard crash (no graceful `shutdown()`) would leave
/// behind, since `shutdown()` itself pauses every runner before persisting
/// and so never writes `.running` back out. A fresh engine over that same
/// store must bring the package back with its progress intact and land the
/// previously-`.running` item somewhere sane, not replay it as still running.
///
/// The item starts disabled so `restore()`'s internal `reconcile()` does not
/// immediately spin up a runner and overwrite the very snapshot fields this
/// test is asserting on — `setEnabled(true, ...)` flips it live afterward, to
/// prove the restored progress is real (the previously "completed" 1000
/// bytes are not backed by any on-disk `.sdmpart`/`.incomplete`, per
/// `DownloadTask.prepare()`'s validator guard, so the resumed run legitimately
/// restarts that item from zero and still has to reach byte identity).
@Test func restoreRepopulatesPackagesWithProgressIntact() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(4000)

    var package = packageOf(["a.bin", "b.bin"])
    let itemID = package.items[0].id
    package.items[0].state = .running
    package.items[0].isEnabled = false
    package.items[0].completed = RangeSet([ByteRange(start: 0, end: 1000)])
    package.items[0].totalBytes = 4000
    package.items[0].isResumable = true

    let store = InMemoryStateStore()
    await store.save(PersistedState(packages: [package]))

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 2,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )
    await engine.restore()

    let restoredPackages = await engine.snapshot().packages
    #expect(restoredPackages.map(\.name) == ["Batch"])
    let restoredItem = try #require(restoredPackages.first?.items.first { $0.id == itemID })
    // Landing state: never `.running` on arrival — nothing in this process is
    // actually running it, and reporting otherwise would misinform both the
    // scheduler and the UI.
    #expect(restoredItem.state == .queued)
    // Progress from the store survives the round trip verbatim (no runner
    // exists for a disabled item, so this reads straight from the restored
    // item rather than a live task).
    #expect(restoredItem.completed.totalBytes == 1000)
    // isResumable resets for re-probing rather than trusting the previous
    // process's answer, which could otherwise make the item unpreemptible
    // before this process has verified anything about the origin.
    #expect(restoredItem.isResumable == nil)

    await engine.setEnabled(true, for: itemID)
    try await engine.runUntilIdle()
    let batch = dir.appendingPathComponent("Batch")
    #expect(try Data(contentsOf: batch.appendingPathComponent("a.bin")) == payload)
    #expect(try Data(contentsOf: batch.appendingPathComponent("b.bin")) == payload)
}

/// `restore()` is idempotent (a second call does not duplicate the restored
/// package) and composes with `add()` called afterward, which is the order
/// `EngineController` actually uses — `restore()` runs once at the head of
/// `startHeartbeat()`, before any user-driven `add()` can occur.
@Test func restoreIsIdempotentAndComposesWithSubsequentAdds() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let store = InMemoryStateStore()
    await store.save(PersistedState(packages: [packageOf(["stored.bin"])]))

    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )

    await engine.restore()
    await engine.restore()  // must not duplicate the first call's work

    let afterRestore = await engine.snapshot().packages.flatMap(\.items).map(\.filename)
    #expect(afterRestore == ["stored.bin"])

    await engine.add(packageOf(["live.bin"]))
    let names = await engine.snapshot().packages.flatMap(\.items).map(\.filename).sorted()
    #expect(names == ["live.bin", "stored.bin"])
}

@Test func snapshotItemCarriesItsSourceURL() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = makeEngine(payload: testPayload(100), folder: dir)

    let url = URL(string: "https://example.com/a.bin")!
    await engine.add(
        DownloadPackage(name: "Pkg", items: [DownloadItem(url: url, filename: "a.bin")]))

    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first?.items.first?.url == url)
}
