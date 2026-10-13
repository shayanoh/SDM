import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine

private func wholesaleItem() -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(
                url: URL(string: "https://www.tiktok.com/@a/video/1")!,
                partFilename: "Clip.mp4", totalBytes: nil,
                origin: .wholesale(formatSelector: "bv*+ba/b"), isResumable: false)
        ],
        outputFilename: "Clip.mp4",
        sourceURL: URL(string: "https://www.tiktok.com/@a/video/1")!,
        assembly: .none, state: .queued)
}

private func wholesaleEngine(
    dir: URL, downloader: FakeWholesaleDownloader, maxConcurrent: Int = 1
) -> DownloadEngine {
    DownloadEngine(
        transport: FakeOrigin(payload: Data()),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: maxConcurrent, segmentsPerItem: 1, globalMaxConnections: 8,
            downloadFolder: dir),
        wholesaleDownloader: downloader)
}

@Test func wholesaleDownloadReportsSynthesizedProgressThenCompletes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()

    fake.emit(WholesaleProgress(downloadedBytes: 500, totalBytes: 1000, phase: .downloading))
    var snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .running)
    #expect(snap?.completed.totalBytes == 500)
    #expect(snap?.totalBytes == 1000)
    #expect(snap?.isResumable == false)

    fake.emit(WholesaleProgress(phase: .postProcessing))
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.isAssembling == true)

    fake.finishSuccess()
    try await engine.runUntilIdle()
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .completed)
    #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/Clip.mp4").path))
}

@Test func hlsFragmentProgressYieldsAMovingTotalAndMonotonicBar() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()

    // HLS: no real total_bytes; fraction comes from fragment index/count.
    // total is derived as downloaded / fraction and moves each report.
    fake.emit(WholesaleProgress(downloadedBytes: 1_000_000, fraction: 0.10))
    var snap = await snapshotItem(item.id, in: engine)
    let total1 = snap?.totalBytes
    let frac1 = Double(snap?.completed.totalBytes ?? 0) / Double(total1 ?? 1)
    #expect(total1 == 10_000_000)
    #expect(abs(frac1 - 0.10) < 0.001)

    fake.emit(WholesaleProgress(downloadedBytes: 3_000_000, fraction: 0.25))
    snap = await snapshotItem(item.id, in: engine)
    let total2 = snap?.totalBytes
    let frac2 = Double(snap?.completed.totalBytes ?? 0) / Double(total2 ?? 1)
    #expect(total2 == 12_000_000)  // estimate changed
    #expect(total2 != total1)
    #expect(frac2 > frac1)  // bar advanced

    fake.finishSuccess()
    try await engine.runUntilIdle()
    #expect(await snapshotItem(item.id, in: engine)?.state == .completed)
}

@Test func pausingANonResumableWholesaleDownloadShowsZeroAndReinvokes() async throws {
    // No fragmented progress seen ⇒ the resumable native downloader is not
    // confirmed ⇒ the item stays non-resumable and its bar shows zero on
    // pause (its next attempt cannot rely on --continue).
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()
    fake.emit(WholesaleProgress(downloadedBytes: 400, totalBytes: 1000, phase: .downloading))

    await engine.stopItem(item.id)
    try await engine.runUntilIdle()
    var snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .stopped)
    #expect(snap?.isResumable == false)
    #expect(snap?.completed.totalBytes == 0)

    #expect(fake.callCount == 1)
    await engine.startItem(item.id)
    await fake.waitUntilStarted()
    #expect(fake.callCount == 2)
    fake.finishSuccess()
    try await engine.runUntilIdle()
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .completed)
}

@Test func fragmentedProgressMakesAWholesaleItemResumable() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()

    #expect(await snapshotItem(item.id, in: engine)?.isResumable == false)

    fake.emit(
        WholesaleProgress(
            downloadedBytes: 400, totalBytes: 1000, fraction: 0.4,
            phase: .downloading, isFragmented: true))
    await engine.tick()  // mirrors the probe result onto the item
    #expect(await snapshotItem(item.id, in: engine)?.isResumable == true)

    fake.finishSuccess()
    try await engine.runUntilIdle()
}

@Test func pausingAResumableWholesaleDownloadKeepsProgressAndResumes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()
    fake.emit(
        WholesaleProgress(
            downloadedBytes: 400, totalBytes: 1000, fraction: 0.4,
            phase: .downloading, isFragmented: true))
    await engine.tick()

    await engine.stopItem(item.id)
    try await engine.runUntilIdle()
    var snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .stopped)
    #expect(snap?.isResumable == true)
    // Synthesized progress is retained — the next run's --continue resumes.
    #expect(snap?.completed.totalBytes == 400)
    #expect(fake.callCount == 1)

    await engine.startItem(item.id)
    await fake.waitUntilStarted()
    #expect(fake.callCount == 2)
    fake.finishSuccess()
    try await engine.runUntilIdle()
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .completed)
}

@Test func aResumableWholesaleItemIsPreemptedByAHigherPriorityItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        wholesaleDownloader: fake)

    let hls = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [hls]))
    await fake.waitUntilStarted()
    fake.emit(
        WholesaleProgress(
            downloadedBytes: 400, totalBytes: 1000, fraction: 0.4,
            phase: .downloading, isFragmented: true))
    await engine.tick()
    #expect(await snapshotItem(hls.id, in: engine)?.isResumable == true)

    // A higher-priority plain item arrives; the wholesale item is now
    // resumable, so once hysteresis elapses it yields its slot rather than
    // holding it as a non-resumable item would.
    let plain = DownloadItem(
        url: URL(string: "https://example.com/b.bin")!, filename: "b.bin",
        priority: .highest)
    await engine.add(DownloadPackage(name: "Q", items: [plain]))
    for _ in 0..<(AppTiming.ticksPerSecond * 5) { await engine.tick() }

    #expect(await snapshotItem(hls.id, in: engine)?.state == .queued)
    #expect(await snapshotItem(plain.id, in: engine)?.state == .running)
    #expect(fake.callCount == 1)

    // Let plain finish; the wholesale item is re-desired and re-invoked
    // (its --continue resume), which we then let finish.
    let resumeWatch = Task {
        while fake.callCount < 2 { await Task.yield() }
        fake.finishSuccess()
    }
    await gate.open()
    try await engine.runUntilIdle()
    await resumeWatch.value
    #expect(fake.callCount == 2)
    #expect(await snapshotItem(hls.id, in: engine)?.state == .completed)
}

@Test func aRunningWholesaleItemReservesItsSchedulerSlot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake, maxConcurrent: 1)

    let hls = wholesaleItem()
    let plain = DownloadItem(url: testSourceURL, filename: "b.bin")
    await engine.add(DownloadPackage(name: "P", items: [hls, plain]))
    await fake.waitUntilStarted()

    // The wholesale item is non-resumable, so pass 1 keeps its slot even
    // though `plain` outranks nothing here — it must not be preempted.
    await engine.tick()
    let hlsSnap = await snapshotItem(hls.id, in: engine)
    let plainSnap = await snapshotItem(plain.id, in: engine)
    #expect(hlsSnap?.state == .running)
    #expect(plainSnap?.state == .queued)

    fake.finishSuccess()
    try await engine.runUntilIdle()
}

@Test func aPermanentWholesaleErrorFailsTheItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()

    fake.finishFailure(.authRequired)
    try await engine.runUntilIdle()
    let snap = await snapshotItem(item.id, in: engine)
    guard case .failed = snap?.state else {
        Issue.record("expected .failed, got \(String(describing: snap?.state))")
        return
    }
}
