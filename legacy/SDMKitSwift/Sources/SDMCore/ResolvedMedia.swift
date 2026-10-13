import Foundation

public struct ResolvedMedia: Sendable, Codable, Equatable {
    public var extractor: String
    public var videoID: String
    public var title: String
    public var durationSeconds: Double?
    public var formats: [MediaFormat]
    /// The page URL this entry resolves from. Set for not-yet-resolved
    /// playlist entries (so the grabber re-resolves each against its own
    /// URL rather than a synthesized one); `nil` for a directly resolved
    /// single. Parent spec `2026-09-03-multi-site-resolver-design.md` §5.3.
    public var sourceURL: URL?

    public init(
        extractor: String, videoID: String, title: String,
        durationSeconds: Double?, formats: [MediaFormat], sourceURL: URL? = nil
    ) {
        self.extractor = extractor
        self.videoID = videoID
        self.title = title
        self.durationSeconds = durationSeconds
        self.formats = formats
        self.sourceURL = sourceURL
    }
}

public enum ResolvedTarget: Sendable {
    case single(ResolvedMedia)
    case playlist(title: String, entries: [ResolvedMedia], totalAvailable: Int)
}
