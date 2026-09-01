import Foundation

public struct ResolvedMedia: Sendable, Codable, Equatable {
    public var extractor: String
    public var videoID: String
    public var title: String
    public var durationSeconds: Double?
    public var formats: [MediaFormat]

    public init(
        extractor: String, videoID: String, title: String,
        durationSeconds: Double?, formats: [MediaFormat]
    ) {
        self.extractor = extractor
        self.videoID = videoID
        self.title = title
        self.durationSeconds = durationSeconds
        self.formats = formats
    }
}

public enum ResolvedTarget: Sendable {
    case single(ResolvedMedia)
    case playlist(title: String, entries: [ResolvedMedia], totalAvailable: Int)
}
