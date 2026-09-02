import Foundation
import OSLog
import SDMCore

/// Logs every yt-dlp invocation and its full stderr. `log stream
/// --predicate 'subsystem == "com.sdm.SDMResolve"'` in Console/Terminal.
let resolveLog = Logger(subsystem: "com.sdm.SDMResolve", category: "yt-dlp")

/// yt-dlp as a metadata extractor only — never as a downloader. Parent
/// spec §4.4. All subprocess access goes through the injected `ProcessRunner`.
public struct YtDlpResolver: LinkResolver {
    public static let handledHosts: Set<String> = [
        "youtube.com", "www.youtube.com", "m.youtube.com",
        "music.youtube.com", "youtu.be",
    ]

    private let runner: any ProcessRunner
    private let locator: BinaryLocator
    private let cookieSource: @Sendable () -> CookieSource
    private let maxPlaylistVideos: @Sendable () -> Int
    private let resolveTimeout: Duration
    private let playlistTimeout: Duration

    public init(
        runner: any ProcessRunner,
        locator: BinaryLocator,
        cookieSource: @escaping @Sendable () -> CookieSource = { .none },
        maxPlaylistVideos: @escaping @Sendable () -> Int = { 50 },
        resolveTimeout: Duration = .seconds(60),
        playlistTimeout: Duration = .seconds(120)
    ) {
        self.runner = runner
        self.locator = locator
        self.cookieSource = cookieSource
        self.maxPlaylistVideos = maxPlaylistVideos
        self.resolveTimeout = resolveTimeout
        self.playlistTimeout = playlistTimeout
    }

    public func canHandle(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
            let host = url.host?.lowercased()
        else { return false }
        return YtDlpResolver.handledHosts.contains(host)
    }

    public func resolve(_ url: URL) async throws -> ResolvedTarget {
        isPlaylistURL(url) ? try await resolvePlaylist(url) : try await resolveSingle(url)
    }

    public func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat {
        guard extractor == "youtube" else { throw ResolveError.unsupported }
        let ytdlp = try await requireYtDlp()
        let canonical = "https://www.youtube.com/watch?v=\(videoID)"
        let args =
            ["-J", "--no-warnings", "--no-playlist"]
            + cookieSource().ytDlpArguments + [canonical]
        let out = try await runYtDlp(ytdlp, args, timeout: resolveTimeout)
        let dump = try decodeDump(out.stdout, context: "-J")
        guard let raw = (dump.formats ?? []).first(where: { $0.formatID == formatID }),
            let mapped = YtDlpParser.mediaFormat(from: raw)
        else { throw ResolveError.formatGone }
        return RefreshedFormat(
            url: mapped.url, filesize: mapped.filesizeEffective, formatID: formatID)
    }

    // MARK: - Playlist detection

    func isPlaylistURL(_ url: URL) -> Bool {
        if let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
            items.contains(where: { $0.name == "list" })
        {
            return true
        }
        return isChannelURL(url) || url.path.lowercased().hasPrefix("/playlist")
    }

    private func isChannelURL(_ url: URL) -> Bool {
        let path = url.path.lowercased()
        return path.hasPrefix("/@") || path.hasPrefix("/channel/")
            || path.hasPrefix("/c/") || path.hasPrefix("/user/") || path.hasSuffix("/videos")
    }

    // MARK: - Resolution

    private func resolveSingle(_ url: URL) async throws -> ResolvedTarget {
        let ytdlp = try await requireYtDlp()
        let args =
            ["-J", "--no-warnings", "--no-playlist"]
            + cookieSource().ytDlpArguments + [url.absoluteString]
        let out = try await runYtDlp(ytdlp, args, timeout: resolveTimeout)
        let dump = try decodeDump(out.stdout, context: "-J")
        return .single(try YtDlpParser.resolvedMedia(from: dump))
    }

    private func resolvePlaylist(_ url: URL) async throws -> ResolvedTarget {
        let ytdlp = try await requireYtDlp()
        let args =
            ["-J", "--flat-playlist", "--no-warnings"]
            + cookieSource().ytDlpArguments + [url.absoluteString]
        let out = try await runYtDlp(ytdlp, args, timeout: playlistTimeout)
        let dump = try decodeDump(out.stdout, context: "--flat-playlist")
        let all = YtDlpParser.flatEntries(from: dump)
        guard !all.isEmpty else { throw ResolveError.unavailable }
        let cap = max(10, min(200, maxPlaylistVideos()))
        let kept = isChannelURL(url) ? Array(all.prefix(cap)) : Array(all.suffix(cap))
        let entries = kept.map {
            ResolvedMedia(
                extractor: "youtube", videoID: $0.videoID, title: $0.title,
                durationSeconds: nil, formats: [])
        }
        return .playlist(
            title: dump.title ?? "Playlist", entries: entries, totalAvailable: all.count)
    }

    // MARK: - Helpers

    private func requireYtDlp() async throws -> URL {
        guard let ytdlp = await locator.locate("yt-dlp") else { throw ResolveError.binaryMissing }
        return ytdlp
    }

    private func runYtDlp(
        _ executable: URL, _ arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        resolveLog.debug(
            "run \(executable.path, privacy: .public) \(arguments.joined(separator: " "), privacy: .public)"
        )
        let out: ProcessOutput
        do {
            out = try await runner.run(
                executable: executable, arguments: arguments, timeout: timeout)
        } catch ProcessRunError.timedOut {
            resolveLog.error(
                "yt-dlp timed out: \(arguments.joined(separator: " "), privacy: .public)")
            throw ResolveError.timeout
        }
        guard out.exitCode == 0 else {
            let stderr = String(decoding: out.stderr, as: UTF8.self)
            // Always log the real, full stderr — this is the only place it
            // exists verbatim before it is classified/truncated.
            resolveLog.error(
                "yt-dlp exit \(out.exitCode, privacy: .public) for [\(arguments.joined(separator: " "), privacy: .public)]\nstderr:\n\(stderr, privacy: .public)"
            )
            throw Classifier.error(fromStderr: stderr, exitCode: out.exitCode)
        }
        return out
    }

    private func decodeDump(_ data: Data, context: String) throws -> YtDlpDump {
        do {
            return try JSONDecoder().decode(YtDlpDump.self, from: data)
        } catch {
            throw ResolveError.ytDlpFailed(stderrTail: "unparseable \(context) output")
        }
    }

    enum Classifier {
        static func error(fromStderr stderr: String, exitCode: Int32) -> ResolveError {
            // Normalize typographic apostrophes (yt-dlp uses U+2019) so the
            // "you're not a bot" match actually fires.
            let lower = stderr.lowercased()
                .replacingOccurrences(of: "\u{2019}", with: "'")

            // YouTube served an anti-bot / consent wall with no session at
            // all — the fix is to provide cookies.
            if lower.contains("confirm you're not a bot")
                || lower.contains("sign in to confirm you're not a bot")
                || lower.contains("sign in to confirm your age")
                || (lower.contains("sign in") && lower.contains("cookies"))
            {
                return .authRequired
            }
            if lower.contains("private video") { return .privateVideo }
            if lower.contains("video unavailable")
                || lower.contains("this video is not available")
                || lower.contains("has been removed")
            {
                return .unavailable
            }
            // Everything else: keep a generous slice of the *real* stderr so
            // it can reach the row's tooltip.
            return .ytDlpFailed(
                stderrTail: String(
                    stderr.trimmingCharacters(in: .whitespacesAndNewlines).suffix(2000)))
        }
    }
}
