import Foundation

public struct QualityPreferences: Sendable, Codable, Equatable {
    public var maxHeight: Int
    public var videoCodecs: Set<VideoCodec>
    public var containers: Set<MediaContainer>
    public var audioCodecs: Set<AudioCodec>

    public init(
        maxHeight: Int, videoCodecs: Set<VideoCodec>,
        containers: Set<MediaContainer>, audioCodecs: Set<AudioCodec>
    ) {
        self.maxHeight = maxHeight
        self.videoCodecs = videoCodecs
        self.containers = containers
        self.audioCodecs = audioCodecs
    }

    public static let `default` = QualityPreferences(
        maxHeight: 1080,
        videoCodecs: [.av1, .vp9, .h264],
        containers: [.mp4, .webm],
        audioCodecs: [.opus, .aac])
}

public struct FormatChoice: Sendable, Codable, Equatable {
    public var video: MediaFormat?
    public var audio: MediaFormat?
    public var outputContainer: MediaContainer
    public var estimatedBytes: Int64?
    /// Non-nil ⇒ this choice is a wholesale (yt-dlp-as-downloader) job for
    /// an HLS/DASH-only stream. The string is a yt-dlp `-f` selector.
    /// `video`/`audio` stay populated for the picker label; the engine
    /// ignores them for a wholesale component. Parent spec §6.2.
    public var wholesaleSelector: String?

    public init(
        video: MediaFormat?, audio: MediaFormat?,
        outputContainer: MediaContainer, estimatedBytes: Int64?,
        wholesaleSelector: String? = nil
    ) {
        self.video = video
        self.audio = audio
        self.outputContainer = outputContainer
        self.estimatedBytes = estimatedBytes
        self.wholesaleSelector = wholesaleSelector
    }

    public var isWholesale: Bool { wholesaleSelector != nil }

    public var requiresMux: Bool {
        video != nil && audio != nil && wholesaleSelector == nil
    }

    public var formatIDs: [String] {
        [video?.id, audio?.id].compactMap { $0 }
    }
}
