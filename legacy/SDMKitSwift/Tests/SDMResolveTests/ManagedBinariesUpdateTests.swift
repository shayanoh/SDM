import CryptoKit
import Foundation
import Testing

@testable import SDMResolve

private func sha256Hex(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

/// A fetcher primed for a successful stable-channel yt-dlp download of `tag`.
private func primedFetcher(tag: String, binary: Data) -> FakeBinaryFetcher {
    let f = FakeBinaryFetcher()
    let ch = YtDlpChannel.stable
    f.setResponse(.success(Data("{\"tag_name\":\"\(tag)\"}".utf8)), for: ch.releasesAPI)
    f.setResponse(.success(binary), for: ch.assetURL(tag: tag))
    f.setResponse(
        .success(Data("\(sha256Hex(binary))  yt-dlp_macos\n".utf8)), for: ch.sumsURL(tag: tag))
    return f
}

private func makeManaged(
    bin: URL, fetcher: FakeBinaryFetcher, runner: FakeProcessRunner = FakeProcessRunner(),
    channel: @escaping @Sendable () -> YtDlpChannel = { .stable },
    onChanged: @escaping @Sendable () async -> Void = {}
) -> ManagedBinaries {
    ManagedBinaries(
        binDirectory: bin, fetcher: fetcher, runner: runner,
        vendorAssets: { [] }, channel: channel, onBinariesChanged: onChanged)
}

@Test func freshInstallDownloadsYtDlp() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let payload = Data("YTDLP-BINARY-2026.08.19".utf8)
    let fetcher = primedFetcher(tag: "2026.08.19", binary: payload)

    let changed = Counter()
    let mb = makeManaged(bin: bin, fetcher: fetcher, onChanged: { await changed.bump() })
    let outcome = await mb.checkNow(reason: .launch)

    #expect(outcome == .updated("2026.08.19"))
    let installed = bin.appendingPathComponent("yt-dlp")
    #expect(FileManager.default.isExecutableFile(atPath: installed.path))
    #expect(try Data(contentsOf: installed) == payload)
    #expect(await mb.manifestForTesting.ytDlpVersion == "2026.08.19")
    #expect(await changed.value == 1)
}

@Test func upToDateSkipsDownload() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let fetcher = primedFetcher(tag: "2026.08.19", binary: Data("x".utf8))
    let mb = makeManaged(bin: bin, fetcher: fetcher)
    _ = await mb.checkNow(reason: .launch)  // installs
    let countAfterInstall = fetcher.requestCount

    let outcome = await mb.checkNow(reason: .timer)
    #expect(outcome == .upToDate("2026.08.19"))
    #expect(fetcher.requestCount == countAfterInstall + 1)  // only the release-info call
}

@Test func newerRemoteReplacesBinary() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")

    let mb1 = makeManaged(
        bin: bin, fetcher: primedFetcher(tag: "2026.08.19", binary: Data("old".utf8)))
    _ = await mb1.checkNow(reason: .launch)

    let mb2 = makeManaged(
        bin: bin, fetcher: primedFetcher(tag: "2026.09.01", binary: Data("new".utf8)))
    let outcome = await mb2.checkNow(reason: .timer)
    #expect(outcome == .updated("2026.09.01"))
    #expect(try Data(contentsOf: bin.appendingPathComponent("yt-dlp")) == Data("new".utf8))
}

@Test func nightlyTagBeatsStableDate() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let mb1 = makeManaged(
        bin: bin, fetcher: primedFetcher(tag: "2026.08.19", binary: Data("a".utf8)))
    _ = await mb1.checkNow(reason: .launch)

    let f2 = FakeBinaryFetcher()
    let ch = YtDlpChannel.stable
    let payload = Data("b".utf8)
    f2.setResponse(.success(Data("{\"tag_name\":\"2026.08.19.120000\"}".utf8)), for: ch.releasesAPI)
    f2.setResponse(.success(payload), for: ch.assetURL(tag: "2026.08.19.120000"))
    f2.setResponse(
        .success(Data("\(sha256Hex(payload))  yt-dlp_macos\n".utf8)),
        for: ch.sumsURL(tag: "2026.08.19.120000"))
    let mb2 = makeManaged(bin: bin, fetcher: f2)
    #expect(await mb2.checkNow(reason: .timer) == .updated("2026.08.19.120000"))
}

@Test func checksumMismatchAborts() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
    let sentinel = bin.appendingPathComponent("yt-dlp")
    try Data("SENTINEL".utf8).write(to: sentinel)
    try FileManager.default.setAttributes(
        [.posixPermissions: 0o755], ofItemAtPath: sentinel.path)

    let f = FakeBinaryFetcher()
    let ch = YtDlpChannel.stable
    f.setResponse(.success(Data("{\"tag_name\":\"2026.09.01\"}".utf8)), for: ch.releasesAPI)
    f.setResponse(.success(Data("corrupt".utf8)), for: ch.assetURL(tag: "2026.09.01"))
    f.setResponse(
        .success(Data("deadbeef  yt-dlp_macos\n".utf8)), for: ch.sumsURL(tag: "2026.09.01"))

    let mb = makeManaged(bin: bin, fetcher: f)
    let outcome = await mb.checkNow(reason: .timer)
    if case .failed = outcome {} else { Issue.record("expected .failed, got \(outcome)") }
    #expect(try Data(contentsOf: sentinel) == Data("SENTINEL".utf8))
    #expect(await mb.manifestForTesting.lastError != nil)
}

@Test func offlineReportsNoNetwork() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let f = FakeBinaryFetcher()
    f.setResponse(
        .failure(BinaryFetchError.transport("offline")), for: YtDlpChannel.stable.releasesAPI)
    let mb = makeManaged(bin: bin, fetcher: f)
    #expect(await mb.checkNow(reason: .launch) == .noNetwork)
    #expect(await mb.manifestForTesting.lastError != nil)
}

@Test func concurrentChecksCoalesce() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let fetcher = primedFetcher(tag: "2026.08.19", binary: Data("x".utf8))
    let mb = makeManaged(bin: bin, fetcher: fetcher)

    async let a = mb.checkNow(reason: .launch)
    async let b = mb.checkNow(reason: .timer)
    async let c = mb.checkNow(reason: .manual)
    async let d = mb.checkNow(reason: .resolveNeeded)
    async let e = mb.checkNow(reason: .timer)
    _ = await (a, b, c, d, e)

    // release-info + asset + sums == 3 requests for a single download.
    #expect(fetcher.requestCount == 3)
}

private actor Counter {
    private(set) var value = 0
    func bump() { value += 1 }
}
