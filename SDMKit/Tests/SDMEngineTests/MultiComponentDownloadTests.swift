import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine

private func twoComponentItem(_ videoURL: URL, _ audioURL: URL) -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(url: videoURL, partFilename: "clip.f137.mp4", origin: .http),
            FileComponent(url: audioURL, partFilename: "clip.f251.webm", origin: .http),
        ],
        outputFilename: "clip.mp4", assembly: .none, state: .queued)
}

@Test func twoComponentsDownloadInParallelAndBothPartsMatchTheirSources() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(6000)
    let audioPayload = Data(testPayload(1500).map { $0 ^ 0x5A })
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: videoPayload,
        audioHost: "a.example", audioPayload: audioPayload)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                twoComponentItem(
                    URL(string: "https://v.example/f137")!, URL(string: "https://a.example/f251")!)
            ]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f137.mp4")) == videoPayload)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f251.webm")) == audioPayload)
}

@Test func oneComponentPermanentlyFailingFailsTheWholeItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var badBehavior = FakeOrigin.Behavior()
    badBehavior.statusOverride = 404
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(6000),
        audioHost: "a.example", audioPayload: testPayload(1500), audioBehavior: badBehavior)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                twoComponentItem(
                    URL(string: "https://v.example/f137")!, URL(string: "https://a.example/f251")!)
            ]))
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    if case .failed = snap.state {} else { Issue.record("expected .failed, got \(snap.state)") }
}

@Test(arguments: [1, 7, 42, 128, 999])
func concatenatedProgressStaysWellFormedUnderChurn(seed: Int) async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(20_000)
    let audioPayload = testPayload(4000)
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: videoPayload,
        audioHost: "a.example", audioPayload: audioPayload)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 8, globalMaxConnections: 32, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                DownloadItem(
                    components: [
                        FileComponent(
                            url: URL(string: "https://v.example/v")!, partFilename: "c.f137.mp4"),
                        FileComponent(
                            url: URL(string: "https://a.example/a")!, partFilename: "c.f251.webm"),
                    ], outputFilename: "c.mp4", assembly: .none, state: .queued)
            ]))

    var rng = SeededGenerator(seed: UInt64(seed))
    for _ in 0..<40 {
        await engine.tick()
        if Bool.random(using: &rng) {
            await engine.updateSettings(
                EngineSettings(
                    maxConcurrent: 1, segmentsPerItem: Int.random(in: 1...8, using: &rng),
                    globalMaxConnections: 32, downloadFolder: dir))
        }
        let snap = await engine.snapshot().packages.first!.items.first!
        var previousEnd: Int64 = -1
        for range in snap.completed.ranges {
            #expect(range.start >= 0)
            #expect(range.start > previousEnd)  // sorted, disjoint, coalesced
            #expect(range.end > range.start)
            previousEnd = range.end
            if let total = snap.totalBytes { #expect(range.end <= total) }
        }
    }
    try await engine.runUntilIdle()
    let snap = await engine.snapshot().packages.first!.items.first!
    #expect(snap.state == .completed)
    #expect(snap.completed.totalBytes == 24_000)
}

/// Routes the video host to a gated origin (body withheld until the gate
/// opens) and the audio host to a normal one, so we can freeze the snapshot
/// with audio complete and video at zero.
private struct GatedVideoRouter: HTTPTransport {
    let video: WorkerGatedOrigin
    let audio: FakeOrigin
    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        request.url.host == "v.example"
            ? try await video.fetch(request) : try await audio.fetch(request)
    }
}

@Test func concatenatedSnapshotAccountsForBothComponentsNotJustTheLarger() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(10_000)
    let audioPayload = testPayload(2_000)
    let gate = WorkerGate()
    let router = GatedVideoRouter(
        video: WorkerGatedOrigin(payload: videoPayload, gate: gate),
        audio: FakeOrigin(payload: audioPayload))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                twoComponentItem(
                    URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)
            ]))

    // Audio (ungated) finishes; video is stuck at zero behind the gate.
    var spins = 0
    while await engine.snapshot().packages.first?.items.first?.completed.totalBytes ?? 0 < 2_000,
        spins < 100_000
    {
        await Task.yield()
        spins += 1
    }
    let mid = try #require(await engine.snapshot().packages.first?.items.first)
    // Total is the sum; progress reflects the finished audio (2000), not
    // capped at some single component's size or lost to a base-0 overlap.
    #expect(mid.totalBytes == 12_000)
    #expect(mid.completed.totalBytes == 2_000)
    // Audio's bytes are placed after the video span, not at offset 0.
    #expect(mid.completed.ranges == [ByteRange(start: 10_000, end: 12_000)])

    await gate.open()
    try await engine.runUntilIdle()
    let done = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(done.completed.totalBytes == 12_000)
    #expect(done.fractionCompleted == 1.0)
}

@Test func resettingAnIncompleteMuxDownloadClearsEveryComponentPartAndSidecar() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let router = GatedVideoRouter(
        video: WorkerGatedOrigin(payload: testPayload(20_000), gate: gate),
        audio: FakeOrigin(payload: testPayload(3_000)))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir,
            checkpointIntervalBytes: 200),
        muxer: FakeMuxer())
    let item = DownloadItem(
        components: [
            FileComponent(url: URL(string: "https://v.example/v")!, partFilename: "clip.f137.mp4"),
            FileComponent(url: URL(string: "https://a.example/a")!, partFilename: "clip.f251.webm"),
        ], outputFilename: "clip.mp4", assembly: .mux, state: .queued)
    await engine.add(DownloadPackage(name: "P", items: [item]))

    // Wait for the audio component to finish (its part file finalizes) and
    // video to have a sidecar, then tick so checkpoints land.
    var spins = 0
    let audioFinal = dir.appendingPathComponent("P/clip.f251.webm")
    while !FileManager.default.fileExists(atPath: audioFinal.path), spins < 200_000 {
        await Task.yield()
        spins += 1
    }
    for _ in 0..<3 { await engine.tick() }
    let videoIncomplete = SparseFile.incompleteURL(
        for: dir.appendingPathComponent("P/clip.f137.mp4"))
    #expect(FileManager.default.fileExists(atPath: videoIncomplete.path))

    await engine.resetDownload(item.id)

    // Everything the item wrote is gone.
    for name in ["clip.mp4", "clip.f137.mp4", "clip.f251.webm"] {
        let base = dir.appendingPathComponent("P/\(name)")
        #expect(!FileManager.default.fileExists(atPath: base.path))
        #expect(!FileManager.default.fileExists(atPath: SparseFile.incompleteURL(for: base).path))
        #expect(!FileManager.default.fileExists(atPath: ResumeSidecar.url(for: base).path))
    }

    // And it resumes cleanly to a hash-matching pair of parts.
    await gate.open()
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
}

@Test func snapshotExposesEveryComponentPartFilename() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(1000),
        audioHost: "a.example", audioPayload: testPayload(300))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                twoComponentItem(
                    URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!),
                DownloadItem(url: URL(string: "https://x/f.bin")!, filename: "f.bin"),
            ]))
    let items = await engine.snapshot().packages.first!.items
    #expect(items[0].partFilenames == ["clip.f137.mp4", "clip.f251.webm"])
    #expect(items[1].partFilenames == ["f.bin"])
}

@Test func aSingleComponentItemWhosePartNameDiffersIsRenamedToTheOutputName() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(3000)
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: payload), stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir))
    // A stale-shaped progressive item: part file "clip.f18.mp4", output
    // "clip.mp4", assembly .none.
    let item = DownloadItem(
        components: [
            FileComponent(
                url: URL(string: "https://gv/prog")!, partFilename: "clip.f18.mp4",
                origin: .resolved(formatID: "18"))
        ], outputFilename: "clip.mp4", assembly: .none, state: .queued)
    await engine.add(DownloadPackage(name: "P", items: [item]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(!snap.fileMissing)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.mp4")) == payload)
    #expect(
        !FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f18.mp4").path))
}
