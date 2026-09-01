import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine

/// Records mux calls; scriptable to succeed (writing a stub output) or fail.
final class FakeMuxer: Muxer, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [(video: URL, audio: URL, output: URL)] = []
    var calls: [(video: URL, audio: URL, output: URL)] { lock.withLock { _calls } }
    var behavior: @Sendable (URL) throws -> Void = { output in
        try Data("MUXED".utf8).write(to: output)
    }

    func mux(
        videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer
    ) async throws {
        lock.withLock { _calls.append((videoPart, audioPart, output)) }
        try behavior(output)
    }
}

private func muxItem(_ v: URL, _ a: URL) -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(url: v, partFilename: "clip.f137.mp4", origin: .http),
            FileComponent(url: a, partFilename: "clip.f251.webm", origin: .http),
        ], outputFilename: "clip.mp4", assembly: .mux, state: .queued)
}

@Test func muxRunsAfterBothComponentsCompleteThenDeletesParts() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let muxer = FakeMuxer()
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(4000),
        audioHost: "a.example", audioPayload: testPayload(1000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir),
        muxer: muxer)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)
            ]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(muxer.calls.count == 1)
    #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.mp4").path))
    #expect(
        !FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f137.mp4").path))
    #expect(
        !FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f251.webm").path)
    )
}

@Test func muxFailureFailsTheItemKeepsPartsAndCanRetry() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let muxer = FakeMuxer()
    muxer.behavior = { _ in throw MuxError.ffmpegFailed(stderrTail: "Invalid data") }
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(4000),
        audioHost: "a.example", audioPayload: testPayload(1000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir),
        muxer: muxer)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)
            ]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    guard case .failed(let reason) = snap.state else {
        Issue.record("expected .failed, got \(snap.state)")
        return
    }
    #expect(reason.contains("mux"))
    #expect(
        FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f137.mp4").path))

    muxer.behavior = { output in try Data("MUXED".utf8).write(to: output) }
    await engine.retryMux(snap.id)
    let after = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(after.state == .completed)
    #expect(muxer.calls.count == 2)
}

@Test func muxItemWithoutAMuxerFailsWithAClearReason() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(4000),
        audioHost: "a.example", audioPayload: testPayload(1000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)
            ]))
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    if case .failed = snap.state {} else { Issue.record("expected .failed") }
}
