import Foundation
import Testing

@testable import SDMResolve

@Test func versionComparesComponentWise() {
    #expect(YtDlpVersion("2026.09.01")! > YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("2026.08.19.120000")! > YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("2026.08.19")! == YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("2027.01.01")! > YtDlpVersion("2026.12.31")!)
    #expect(YtDlpVersion("garbage") == nil)
    #expect(YtDlpVersion("") == nil)
}

@Test func manifestRoundTrips() throws {
    var m = BinariesManifest.empty
    m.ytDlpVersion = "2026.08.19"
    m.ytDlpChannel = .nightly
    m.lastError = "boom"
    m.ffmpegVersion = "7.1"
    let data = try JSONEncoder().encode(m)
    #expect(try JSONDecoder().decode(BinariesManifest.self, from: data) == m)
}

@Test func manifestVersionLookupByName() {
    var m = BinariesManifest.empty
    m.ffmpegVersion = "7.1"
    m.qjsVersion = "0.10.0"
    #expect(m.version(for: "ffmpeg") == "7.1")
    #expect(m.version(for: "qjs") == "0.10.0")
    #expect(m.version(for: "yt-dlp") == nil)
}

@Test func channelPicksTheRightRepo() {
    #expect(
        YtDlpChannel.stable.releasesAPI.absoluteString
            == "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
    #expect(
        YtDlpChannel.nightly.releasesAPI.absoluteString
            == "https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest")
    #expect(
        YtDlpChannel.stable.assetURL(tag: "2026.08.19").absoluteString
            == "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos")
    #expect(
        YtDlpChannel.stable.sumsURL(tag: "2026.08.19").absoluteString
            == "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/SHA2-256SUMS")
}
