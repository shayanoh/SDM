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
}
