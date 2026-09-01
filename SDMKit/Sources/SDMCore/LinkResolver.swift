import Foundation

public struct RefreshedFormat: Sendable, Equatable {
    public var url: URL
    public var filesize: Int64?
    public var formatID: String

    public init(url: URL, filesize: Int64?, formatID: String) {
        self.url = url
        self.filesize = filesize
        self.formatID = formatID
    }
}

public enum ResolveError: Error, Equatable, Sendable {
    case binaryMissing
    case unsupported
    case formatGone
    case authRequired
    case privateVideo
    case unavailable
    case timeout
    case ytDlpFailed(stderrTail: String)
}

/// The extension seam for extractor-backed sites. Generic HTTP has no
/// resolver — a `nil` resolver, or `canHandle == false`, means "use the
/// existing probe/download path." Parent spec §4.1.
public protocol LinkResolver: Sendable {
    /// Cheap, synchronous, no I/O — a host/path check only.
    func canHandle(_ url: URL) -> Bool

    /// Grab-time resolution. May shell out; may take seconds.
    func resolve(_ url: URL) async throws -> ResolvedTarget

    /// Mid-download URL refresh for one expired component.
    func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat
}
