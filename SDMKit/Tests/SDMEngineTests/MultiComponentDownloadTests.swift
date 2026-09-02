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
