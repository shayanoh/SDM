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

@Test func pausingAWholesaleDownloadDiscardsPartialAndRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = wholesaleEngine(dir: dir, downloader: fake)
    let item = wholesaleItem()
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await fake.waitUntilStarted()
    fake.emit(WholesaleProgress(downloadedBytes: 400, totalBytes: 1000, phase: .downloading))

    await engine.stopItem(item.id)
    // Let the non-blocking pause unwind.
    try await engine.runUntilIdle()
    var snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .stopped)
    #expect(snap?.completed.totalBytes == 0)
    #expect(
        !FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/Clip.mp4").path))

    #expect(fake.callCount == 1)
    await engine.startItem(item.id)
    await fake.waitUntilStarted()
    #expect(fake.callCount == 2)
    fake.finishSuccess()
    try await engine.runUntilIdle()
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .completed)
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
