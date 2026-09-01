import Foundation
import SDMCore

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
        guard let raw = (dump.formats ?? []).first(where: { $0.format_id == formatID }),
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
        let out: ProcessOutput
        do {
            out = try await runner.run(
                executable: executable, arguments: arguments, timeout: timeout)
        } catch ProcessRunError.timedOut {
            throw ResolveError.timeout
        }
        guard out.exitCode == 0 else {
            throw Classifier.error(
                fromStderr: String(decoding: out.stderr, as: UTF8.self), exitCode: out.exitCode)
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
            let lower = stderr.lowercased()
            if lower.contains("sign in to confirm your age")
                || lower.contains("confirm you're not a bot")
                || lower.contains("sign in to confirm you're not a bot")
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
            return .ytDlpFailed(stderrTail: String(stderr.suffix(500)))
        }
    }
}
