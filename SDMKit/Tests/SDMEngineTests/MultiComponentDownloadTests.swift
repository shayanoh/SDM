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
