import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

private func u(_ s: String) -> URL { URL(string: s)! }

private func wholesaleLocator(hasYtDlp: Bool = true) -> BinaryLocator {
    let present: Set<String> = hasYtDlp ? ["/bin/yt-dlp"] : []
    return BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/bin")],
        isExecutable: { present.contains($0.path) })
}

/// Replays a scripted line sequence, then exits with `exitCode`. Optionally
/// creates the destination file (parsed out of `-o <path>`).
private final class FakeStreamingRunner: ProcessRunner, @unchecked Sendable {
    let lines: [String]
    let exitCode: Int32
    let createsOutput: Bool
    private let lock = NSLock()
    private(set) var lastArguments: [String] = []

    init(lines: [String], exitCode: Int32, createsOutput: Bool = true) {
        self.lines = lines
        self.exitCode = exitCode
        self.createsOutput = createsOutput
    }

    func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        ProcessOutput(stdout: Data(), stderr: Data(), exitCode: exitCode)
    }

    func runStreaming(
        executable: URL, arguments: [String], timeout: Duration,
        onLine: @Sendable @escaping (String) -> Void
    ) async throws -> Int32 {
        lock.withLock { lastArguments = arguments }
        for line in lines { onLine(line) }
        if createsOutput, let oIndex = arguments.firstIndex(of: "-o"),
            oIndex + 1 < arguments.count
        {
            FileManager.default.createFile(atPath: arguments[oIndex + 1], contents: Data("x".utf8))
        }
        return exitCode
    }
}

private struct ProgressBox: @unchecked Sendable {
    final class Store: @unchecked Sendable {
        let lock = NSLock()
        var items: [WholesaleProgress] = []
    }
    let store = Store()
    var all: [WholesaleProgress] { store.lock.withLock { store.items } }
    func append(_ p: WholesaleProgress) { store.lock.withLock { store.items.append(p) } }
}

@Test func emitsProgressAndSucceeds() async throws {
    let dest = FileManager.default.temporaryDirectory
        .appendingPathComponent("\(UUID().uuidString).mp4")
    defer { try? FileManager.default.removeItem(at: dest) }
    let runner = FakeStreamingRunner(
        lines: [
            "sdm:downloading|500|1000|NA| 50.0%",
            "sdm:downloading|1000|1000|NA| 100.0%",
            "[Merger] Merging formats into \"o.mp4\"",
        ], exitCode: 0)
    let d = YtDlpWholesaleDownloader(runner: runner, locator: wholesaleLocator())
    let box = ProgressBox()
    try await d.download(
        pageURL: u("https://x.com/1"), formatSelector: "bv*+ba/b", destination: dest,
        onProgress: { box.append($0) })

    #expect(box.all.last?.phase == .postProcessing)
    #expect(box.all.contains { $0.downloadedBytes == 1000 })
    #expect(runner.lastArguments.contains("-f"))
    #expect(runner.lastArguments.contains("bv*+ba/b"))
    #expect(runner.lastArguments.contains("--newline"))
    #expect(runner.lastArguments.contains("https://x.com/1"))
}

@Test func nonZeroExitThrowsFailedWithTail() async {
    let dest = FileManager.default.temporaryDirectory
        .appendingPathComponent("\(UUID().uuidString).mp4")
    let runner = FakeStreamingRunner(
        lines: ["ERROR: [x] boom happened"], exitCode: 1, createsOutput: false)
    let d = YtDlpWholesaleDownloader(runner: runner, locator: wholesaleLocator())
    await #expect(throws: WholesaleError.failed(stderrTail: "ERROR: [x] boom happened")) {
        try await d.download(
            pageURL: u("https://x.com/1"), formatSelector: "b", destination: dest,
            onProgress: { _ in })
    }
}

@Test func drmExitThrowsUnavailable() async {
    let dest = FileManager.default.temporaryDirectory
        .appendingPathComponent("\(UUID().uuidString).mp4")
    let runner = FakeStreamingRunner(
        lines: ["ERROR: This video is DRM protected"], exitCode: 1, createsOutput: false)
    let d = YtDlpWholesaleDownloader(runner: runner, locator: wholesaleLocator())
    await #expect(throws: WholesaleError.unavailable) {
        try await d.download(
            pageURL: u("https://x.com/1"), formatSelector: "b", destination: dest,
            onProgress: { _ in })
    }
}

@Test func missingBinaryThrows() async {
    let d = YtDlpWholesaleDownloader(
        runner: FakeStreamingRunner(lines: [], exitCode: 0),
        locator: wholesaleLocator(hasYtDlp: false))
    await #expect(throws: WholesaleError.binaryMissing) {
        try await d.download(
            pageURL: u("https://x.com/1"), formatSelector: "b",
            destination: URL(fileURLWithPath: "/tmp/o.mp4"), onProgress: { _ in })
    }
}
