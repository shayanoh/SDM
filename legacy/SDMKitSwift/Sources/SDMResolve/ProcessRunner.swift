import Foundation

public struct ProcessOutput: Sendable, Equatable {
    public var stdout: Data
    public var stderr: Data
    public var exitCode: Int32

    public init(stdout: Data, stderr: Data, exitCode: Int32) {
        self.stdout = stdout
        self.stderr = stderr
        self.exitCode = exitCode
    }
}

public enum ProcessRunError: Error, Equatable, Sendable {
    case launchFailed(String)
    case timedOut
}

/// Injected everywhere a subprocess is run, so tests never spawn the real
/// `yt-dlp`/`ffmpeg`. Parent spec §4.5 / §10.2.
public protocol ProcessRunner: Sendable {
    func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput

    /// Runs the child and forwards each completed line of stdout **or**
    /// stderr as it arrives (for progress parsing). Returns the exit code;
    /// a non-zero code is not an error here. Honors task cancellation and
    /// `timeout` by terminating the child (throwing `ProcessRunError` /
    /// `CancellationError`). Parent spec
    /// `2026-09-03-multi-site-resolver-design.md` §6.6.
    func runStreaming(
        executable: URL, arguments: [String], timeout: Duration,
        onLine: @Sendable @escaping (String) -> Void
    ) async throws -> Int32
}

extension ProcessRunner {
    /// Buffered fallback: run to completion, then replay the output
    /// line-by-line. Adequate for fakes and any runner that does not need
    /// true streaming.
    public func runStreaming(
        executable: URL, arguments: [String], timeout: Duration,
        onLine: @Sendable @escaping (String) -> Void
    ) async throws -> Int32 {
        let out = try await run(executable: executable, arguments: arguments, timeout: timeout)
        for chunk in [out.stdout, out.stderr] {
            for line in String(decoding: chunk, as: UTF8.self)
                .split(separator: "\n", omittingEmptySubsequences: true)
            {
                onLine(String(line))
            }
        }
        return out.exitCode
    }
}
