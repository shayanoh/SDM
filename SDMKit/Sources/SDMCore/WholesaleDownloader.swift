import Foundation

/// One progress report from a wholesale (yt-dlp-as-downloader) job.
///
/// `totalBytes` may be nil for a while (some extractors report only an
/// estimate, or nothing until the first fragment). `fraction` is yt-dlp's
/// own percentage, used when byte counts are unavailable. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.5.
public struct WholesaleProgress: Sendable, Equatable {
    public enum Phase: Sendable, Equatable { case downloading, postProcessing }

    public var downloadedBytes: Int64?
    public var totalBytes: Int64?
    public var fraction: Double?
    public var phase: Phase

    public init(
        downloadedBytes: Int64? = nil, totalBytes: Int64? = nil,
        fraction: Double? = nil, phase: Phase = .downloading
    ) {
        self.downloadedBytes = downloadedBytes
        self.totalBytes = totalBytes
        self.fraction = fraction
        self.phase = phase
    }
}

public enum WholesaleError: Error, Equatable, Sendable {
    case binaryMissing
    case cancelled
    case authRequired
    case unavailable
    case failed(stderrTail: String)
}

/// Runs an external downloader (yt-dlp) for a stream that has no single
/// `Range`-capable URL. Injected into `DownloadEngine` exactly like
/// `LinkResolver`; `SDMEngine` never constructs one. Parent spec §6.5.
public protocol WholesaleDownloader: Sendable {
    /// Download `pageURL` wholesale to `destination` — a final file, already
    /// muxed. Emits progress. Honors task cancellation by terminating the
    /// child process; the caller deletes partial output.
    func download(
        pageURL: URL,
        formatSelector: String,
        destination: URL,
        onProgress: @Sendable @escaping (WholesaleProgress) -> Void
    ) async throws
}
