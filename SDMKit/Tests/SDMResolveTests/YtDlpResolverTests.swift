import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

private func makeLocator(hasYtDlp: Bool = true) -> BinaryLocator {
    let present: Set<String> = hasYtDlp ? ["/bin/yt-dlp"] : []
    return BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/bin")], isExecutable: { present.contains($0.path) })
}

private func u(_ s: String) -> URL { URL(string: s)! }

private let ageRestrictedStderr =
    "ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users."
private let privateStderr =
    "ERROR: [youtube] def: Private video. Sign in if you've been granted access to this video"

// MARK: - canHandle

@Test func canHandleUsesSiteRegistry() {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    #expect(r.canHandle(u("https://www.youtube.com/watch?v=abc")))
    #expect(r.canHandle(u("https://youtu.be/abc")))
    #expect(r.canHandle(u("https://music.youtube.com/watch?v=abc")))
    #expect(r.canHandle(u("https://vimeo.com/12345")))
    #expect(r.canHandle(u("https://www.tiktok.com/@a/video/1")))
    #expect(r.canHandle(u("https://www.xvideos.com/video1/x")))
    #expect(!r.canHandle(u("https://example.com/video.mp4")))
    #expect(!r.canHandle(u("ftp://youtube.com/x")))
}

// MARK: - resolve, single video

@Test func resolveSingleVideoReturnsResolvedMedia() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_muxed")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    guard case .single(let media) = target else {
        Issue.record("expected .single")
        return
    }
    #expect(media.videoID == "dQw4w9WgXcQ")
    #expect(media.formats.contains { $0.id == "399" })
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("--no-playlist"))
    #expect(call.arguments.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
}

@Test func resolveWithoutYtDlpThrowsBinaryMissing() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator(hasYtDlp: false))
    await #expect(throws: ResolveError.binaryMissing) {
        try await r.resolve(u("https://youtu.be/abc"))
    }
}

@Test func ageRestrictedVideoThrowsAuthRequired() async {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail(ageRestrictedStderr))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.authRequired) {
        try await r.resolve(u("https://youtu.be/abc"))
    }
}

@Test func privateVideoThrowsPrivateVideo() async {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail(privateStderr))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.privateVideo) {
        try await r.resolve(u("https://youtu.be/def"))
    }
}

@Test func hlsOnlyVideoThrowsUnsupported() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_hls_only")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.unsupported) {
        try await r.resolve(u("https://youtu.be/hls"))
    }
}

@Test func cookieSourceAddsArguments() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_muxed")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), cookieSource: { .firefox })
    _ = try await r.resolve(u("https://youtu.be/abc"))
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("--cookies-from-browser"))
    #expect(call.arguments.contains("firefox"))
}

@Test func runnerTimeoutBecomesResolveTimeout() async {
    let runner = FakeProcessRunner()
    runner.defaultOutput = .failure(ProcessRunError.timedOut)
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.timeout) {
        try await r.resolve(u("https://youtu.be/abc"))
    }
}

// MARK: - resolve, playlists & channels

@Test func detectsPlaylistAndChannelURLs() {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    #expect(r.looksLikePlaylist(u("https://www.youtube.com/playlist?list=PLabc")))
    #expect(r.looksLikePlaylist(u("https://www.youtube.com/watch?v=abc&list=PLabc")))
    #expect(r.looksLikePlaylist(u("https://www.youtube.com/@SomeCreator")))
    #expect(r.looksLikePlaylist(u("https://www.youtube.com/channel/UCxyz/videos")))
    #expect(!r.looksLikePlaylist(u("https://www.youtube.com/watch?v=abc")))
    #expect(!r.looksLikePlaylist(u("https://youtu.be/abc")))
}

@Test func resolvePlaylistReturnsCappedNewestEntries() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 10 })
    let target = try await r.resolve(u("https://www.youtube.com/playlist?list=PLtest"))
    guard case .playlist(let title, let entries, let total) = target else {
        Issue.record("expected .playlist")
        return
    }
    #expect(title == "My Test Playlist")
    #expect(total == 12)
    #expect(entries.count == 10)
    #expect(entries.first?.videoID == "vid03")
    #expect(entries.last?.videoID == "vid12")
    #expect(entries.allSatisfy { $0.formats.isEmpty })
}

@Test func resolveChannelKeepsHeadEntries() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 10 })
    let target = try await r.resolve(u("https://www.youtube.com/@Creator/videos"))
    guard case .playlist(_, let entries, _) = target else {
        Issue.record("expected .playlist")
        return
    }
    #expect(entries.first?.videoID == "vid01")
    #expect(entries.last?.videoID == "vid10")
}

@Test func playlistCapClampsToFloorOfTen() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 3 })
    let target = try await r.resolve(u("https://www.youtube.com/playlist?list=PLtest"))
    guard case .playlist(_, let entries, _) = target else {
        Issue.record("expected .playlist")
        return
    }
    #expect(entries.count == 10)
}

// MARK: - refresh

@Test func refreshReturnsFreshURLForSameFormat() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_refresh_ok")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let refreshed = try await r.refresh(
        sourceURL: u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), formatID: "137")
    #expect(refreshed.formatID == "137")
    #expect(refreshed.filesize == 118_000_000)
    #expect(refreshed.url.absoluteString.contains("sn-FRESH"))
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
}

@Test func refreshThrowsFormatGoneWhenFormatMissing() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_refresh_format_gone")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.formatGone) {
        try await r.refresh(sourceURL: u("https://youtu.be/dQw4w9WgXcQ"), formatID: "137")
    }
}

@Test func refreshWithoutYtDlpThrowsBinaryMissing() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator(hasYtDlp: false))
    await #expect(throws: ResolveError.binaryMissing) {
        try await r.refresh(sourceURL: u("https://youtu.be/x"), formatID: "137")
    }
}

@Test func refreshRejectsAnUnsupportedSourceURL() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    await #expect(throws: ResolveError.unsupported) {
        try await r.refresh(sourceURL: u("https://example.com/12345"), formatID: "1")
    }
}

// MARK: - Classifier

@Test func classifierRecognizesCurlyApostropheBotWall() {
    let stderr =
        "ERROR: [youtube] abc: Sign in to confirm you\u{2019}re not a bot. Use --cookies-from-browser"
    #expect(YtDlpResolver.Classifier.error(fromStderr: stderr, exitCode: 1) == .authRequired)
}

@Test func classifierKeepsPageNeedsReloadingAsRawFailure() {
    let stderr = "ERROR: [youtube] pBLlM8ZvEQo: The page needs to be reloaded.\n"
    let result = YtDlpResolver.Classifier.error(fromStderr: stderr, exitCode: 1)
    guard case .ytDlpFailed(let tail) = result else {
        Issue.record("expected .ytDlpFailed, got \(result)")
        return
    }
    #expect(tail.contains("The page needs to be reloaded"))
}

@Test func classifierUnknownErrorKeepsAGenerousStderrTail() {
    let stderr = String(repeating: "x", count: 5000) + "ERROR: something specific happened"
    guard
        case .ytDlpFailed(let tail) = YtDlpResolver.Classifier.error(
            fromStderr: stderr, exitCode: 1)
    else {
        Issue.record("expected .ytDlpFailed")
        return
    }
    #expect(tail.contains("ERROR: something specific happened"))
    #expect(tail.count <= 2000)
}

// MARK: - extra arguments

@Test func splicesExtraArgumentsIntoSingleResolve() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_muxed")))]
    let r = YtDlpResolver(
        runner: runner, locator: makeLocator(),
        extraArguments: {
            ["--extractor-args", "youtube:jsruntime=quickjs", "--ffmpeg-location", "/x/bin"]
        })
    _ = try await r.resolve(u("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("youtube:jsruntime=quickjs"))
    #expect(call.arguments.contains("/x/bin"))
}

@Test func splicesExtraArgumentsIntoPlaylistAndRefresh() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [
        ("--flat-playlist", ok(try fixtureData("playlist_flat"))),
        ("-J", ok(try fixtureData("video_refresh_ok"))),
    ]
    let r = YtDlpResolver(
        runner: runner, locator: makeLocator(),
        extraArguments: { ["--sponsorblock-mark", "none"] })
    _ = try? await r.resolve(u("https://www.youtube.com/playlist?list=PL123"))
    _ = try? await r.refresh(sourceURL: u("https://www.youtube.com/watch?v=abc"), formatID: "18")
    #expect(runner.calls.count == 2)
    #expect(runner.calls.allSatisfy { $0.arguments.contains("--sponsorblock-mark") })
}
