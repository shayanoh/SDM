import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine
@testable import SDMResolve

private func loc(hasFfmpeg: Bool) -> BinaryLocator {
    let present: Set<String> = hasFfmpeg ? ["/bin/ffmpeg"] : []
    return BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/bin")], isExecutable: { present.contains($0.path) })
}

@Test func ffmpegArgumentsCopyStreamsAndSetFaststartForMp4() {
    let args = FFmpegMuxer.arguments(
        video: URL(fileURLWithPath: "/tmp/v.f137.mp4"),
        audio: URL(fileURLWithPath: "/tmp/a.f140.m4a"),
        output: URL(fileURLWithPath: "/tmp/out.mp4"), container: .mp4)
    #expect(args.contains("-c"))
    #expect(args.contains("copy"))
    #expect(args.contains("+faststart"))
    #expect(args.last == "/tmp/out.mp4")
}

@Test func ffmpegArgumentsOmitFaststartForWebm() {
    let args = FFmpegMuxer.arguments(
        video: URL(fileURLWithPath: "/tmp/v.webm"), audio: URL(fileURLWithPath: "/tmp/a.webm"),
        output: URL(fileURLWithPath: "/tmp/out.webm"), container: .webm)
    #expect(!args.contains("+faststart"))
}

@Test func muxWithoutFfmpegThrowsMissing() async {
    let m = FFmpegMuxer(runner: FakeProcessRunner(), locator: loc(hasFfmpeg: false))
    await #expect(throws: MuxError.ffmpegMissing) {
        try await m.mux(
            videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
            into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
    }
}

@Test func muxSurfacesFfmpegStderrOnFailure() async {
    let runner = FakeProcessRunner()
    runner.defaultOutput = fail("Invalid data found when processing input\n", exitCode: 1)
    let m = FFmpegMuxer(runner: runner, locator: loc(hasFfmpeg: true))
    await #expect(
        throws: MuxError.ffmpegFailed(stderrTail: "Invalid data found when processing input\n")
    ) {
        try await m.mux(
            videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
            into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
    }
}

@Test func muxSucceedsOnZeroExit() async throws {
    let m = FFmpegMuxer(runner: FakeProcessRunner(), locator: loc(hasFfmpeg: true))
    try await m.mux(
        videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
        into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
}

private let ffmpegOnDisk =
    FileManager.default.isExecutableFile(atPath: "/usr/local/bin/ffmpeg")
    || FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/ffmpeg")

@Test(.enabled(if: ffmpegOnDisk))
func realFfmpegMuxesTwoTinyStreams() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let ffmpeg = try #require(await BinaryLocator().locate("ffmpeg"))
    let v = dir.appendingPathComponent("v.mp4")
    let a = dir.appendingPathComponent("a.m4a")
    _ = try await SystemProcessRunner().run(
        executable: ffmpeg,
        arguments: [
            "-y", "-f", "lavfi", "-i", "testsrc=duration=0.2:size=64x64:rate=10",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", v.path,
        ], timeout: .seconds(30))
    _ = try await SystemProcessRunner().run(
        executable: ffmpeg,
        arguments: [
            "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=0.2", "-c:a", "aac", a.path,
        ], timeout: .seconds(30))
    let out = dir.appendingPathComponent("out.mp4")
    try await FFmpegMuxer(runner: SystemProcessRunner(), locator: BinaryLocator())
        .mux(videoPart: v, audioPart: a, into: out, container: .mp4)
    #expect(FileManager.default.fileExists(atPath: out.path))
    #expect((try Data(contentsOf: out)).count > 0)
}
