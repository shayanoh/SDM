import Foundation
import OSLog
import SDMCore

/// Runs `yt-dlp` as a *downloader* for a stream with no single
/// `Range`-capable URL (HLS/DASH). The only place yt-dlp is not just a
/// metadata extractor. Non-resumable by design — the engine discards
/// partial output on any failure. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.6.
public struct YtDlpWholesaleDownloader: WholesaleDownloader {
    private let runner: any ProcessRunner
    private let locator: BinaryLocator
    private let cookieSource: @Sendable () -> CookieSource
    private let extraArguments: @Sendable () -> [String]
    private let timeout: Duration

    public init(
        runner: any ProcessRunner,
        locator: BinaryLocator,
        cookieSource: @escaping @Sendable () -> CookieSource = { .none },
        extraArguments: @escaping @Sendable () -> [String] = { [] },
        timeout: Duration = .seconds(7200)
    ) {
        self.runner = runner
        self.locator = locator
        self.cookieSource = cookieSource
        self.extraArguments = extraArguments
        self.timeout = timeout
    }

    public func download(
        pageURL: URL,
        formatSelector: String,
        destination: URL,
        onProgress: @Sendable @escaping (WholesaleProgress) -> Void
    ) async throws {
        guard let ytdlp = await locator.locate("yt-dlp") else {
            throw WholesaleError.binaryMissing
        }
        let ffmpegDir = ytdlp.deletingLastPathComponent().path
        let container = destination.pathExtension.isEmpty ? "mp4" : destination.pathExtension

        let arguments =
            [
                "-f", formatSelector,
                "--no-playlist",
                "--newline",
                // Resume support: keep the `.part` / `.ytdl` scratch and
                // resume from the last completed fragment on the next run.
                // `--hls-prefer-native` forces yt-dlp's own fragment
                // downloader (the resumable one) over ffmpeg-as-downloader.
                // See `2026-09-03-wholesale-resume-design.md`.
                "--continue",
                "--hls-prefer-native",
                "--no-warnings",
                "--progress-template",
                "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s"
                    + "|%(progress.total_bytes_estimate)s|%(progress.fragment_index)s"
                    + "|%(progress.fragment_count)s",
                "--merge-output-format", container,
                "--ffmpeg-location", ffmpegDir,
                "-o", destination.path,
            ]
            + (SiteRegistry.match(pageURL)?.extraArgs ?? [])
            + cookieSource().ytDlpArguments
            + extraArguments()
            + [pageURL.absoluteString]

        let tail = StderrTail()
        resolveLog.debug(
            "wholesale \(ytdlp.path, privacy: .public) \(arguments.joined(separator: " "), privacy: .public)"
        )

        let exitCode: Int32
        do {
            exitCode = try await runner.runStreaming(
                executable: ytdlp, arguments: arguments, timeout: timeout
            ) { line in
                if let progress = WholesaleProgressParser.parse(line) {
                    onProgress(progress)
                } else {
                    tail.consider(line)
                }
            }
        } catch is CancellationError {
            throw WholesaleError.cancelled
        } catch ProcessRunError.timedOut {
            throw WholesaleError.failed(stderrTail: "yt-dlp timed out")
        }

        guard exitCode == 0 else {
            let stderr = tail.value
            resolveLog.error(
                "wholesale yt-dlp exit \(exitCode, privacy: .public)\n\(stderr, privacy: .public)")
            switch YtDlpResolver.Classifier.error(fromStderr: stderr, exitCode: exitCode) {
            case .authRequired:
                throw WholesaleError.authRequired
            case .privateVideo, .unavailable, .drmProtected:
                throw WholesaleError.unavailable
            default:
                throw WholesaleError.failed(
                    stderrTail: stderr.isEmpty ? "yt-dlp exited \(exitCode)" : stderr)
            }
        }

        guard FileManager.default.fileExists(atPath: destination.path) else {
            throw WholesaleError.failed(stderrTail: "yt-dlp produced no output file")
        }
    }
}

/// Keeps the last ~2 KB of non-progress output for error reporting.
private final class StderrTail: @unchecked Sendable {
    private let lock = NSLock()
    private var lines: [String] = []

    func consider(_ line: String) {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        lock.withLock {
            lines.append(trimmed)
            if lines.count > 60 { lines.removeFirst(lines.count - 60) }
        }
    }

    var value: String {
        lock.withLock { String(lines.joined(separator: "\n").suffix(2000)) }
    }
}
