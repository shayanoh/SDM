import Foundation
import SDMCore
import SDMResolve

public enum MuxError: Error, Equatable, Sendable {
    case ffmpegMissing
    case ffmpegFailed(stderrTail: String)
    case timedOut
}

/// Combines a video-only and audio-only part into one container with
/// `ffmpeg -c copy` (no re-encode). Parent spec §7.2.
public protocol Muxer: Sendable {
    func mux(
        videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer
    ) async throws
}

public struct FFmpegMuxer: Muxer {
    private let runner: any ProcessRunner
    private let locator: BinaryLocator
    private let timeout: Duration

    public init(
        runner: any ProcessRunner, locator: BinaryLocator, timeout: Duration = .seconds(120)
    ) {
        self.runner = runner
        self.locator = locator
        self.timeout = timeout
    }

    public static func arguments(
        video: URL, audio: URL, output: URL, container: MediaContainer
    ) -> [String] {
        var args = [
            "-y", "-i", video.path, "-i", audio.path,
            "-c", "copy", "-map", "0:v:0", "-map", "1:a:0",
        ]
        if container == .mp4 { args += ["-movflags", "+faststart"] }
        args.append(output.path)
        return args
    }

    public func mux(
        videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer
    ) async throws {
        guard let ffmpeg = await locator.locate("ffmpeg") else { throw MuxError.ffmpegMissing }
        let out: ProcessOutput
        do {
            out = try await runner.run(
                executable: ffmpeg,
                arguments: Self.arguments(
                    video: videoPart, audio: audioPart, output: output, container: container),
                timeout: timeout)
        } catch ProcessRunError.timedOut {
            throw MuxError.timedOut
        }
        guard out.exitCode == 0 else {
            throw MuxError.ffmpegFailed(
                stderrTail: String(String(decoding: out.stderr, as: UTF8.self).suffix(800)))
        }
    }
}
