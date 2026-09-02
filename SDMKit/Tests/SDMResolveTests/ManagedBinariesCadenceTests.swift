import CryptoKit
import Foundation
import Testing

@testable import SDMResolve

private func sha256Hex(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

private func tick(_ mb: ManagedBinaries, _ n: Int) async {
    for _ in 0..<n { await mb.tick() }
}

@Test func noCheckBeforePresentInterval() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let payload = Data("ytdlp".utf8)
    let fetcher = FakeBinaryFetcher()
    let ch = YtDlpChannel.stable
    fetcher.setResponse(.success(Data("{\"tag_name\":\"2026.08.19\"}".utf8)), for: ch.releasesAPI)
    fetcher.setResponse(.success(payload), for: ch.assetURL(tag: "2026.08.19"))
    fetcher.setResponse(
        .success(Data("\(sha256Hex(payload))  yt-dlp_macos\n".utf8)),
        for: ch.sumsURL(tag: "2026.08.19"))

    let mb = ManagedBinaries(
        binDirectory: bin, fetcher: fetcher, runner: FakeProcessRunner(),
        vendorAssets: { [] }, channel: { .stable })
    _ = await mb.checkNow(reason: .launch)  // installs; resets the interval clock
    let baseline = fetcher.requestCount

    await tick(mb, ManagedBinaries.presentInterval - 1)
    #expect(fetcher.requestCount == baseline)

    await tick(mb, 1)
    #expect(fetcher.requestCount == baseline + 1)  // one release-info call, then .upToDate
}

@Test func absentRetriesEveryShortInterval() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let fetcher = FakeBinaryFetcher()
    fetcher.setResponse(
        .failure(BinaryFetchError.transport("offline")), for: YtDlpChannel.stable.releasesAPI)
    let mb = ManagedBinaries(
        binDirectory: bin, fetcher: fetcher, runner: FakeProcessRunner(),
        vendorAssets: { [] }, channel: { .stable })

    await tick(mb, ManagedBinaries.absentInterval - 1)
    #expect(fetcher.requestCount == 0)

    await tick(mb, 1)
    #expect(fetcher.requestCount == 1)

    await tick(mb, ManagedBinaries.absentInterval)
    #expect(fetcher.requestCount == 2)
}
