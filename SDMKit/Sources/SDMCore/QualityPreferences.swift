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

    public init(
        video: MediaFormat?, audio: MediaFormat?,
        outputContainer: MediaContainer, estimatedBytes: Int64?
    ) {
        self.video = video
        self.audio = audio
        self.outputContainer = outputContainer
        self.estimatedBytes = estimatedBytes
    }

    public var requiresMux: Bool { video != nil && audio != nil }

    public var formatIDs: [String] {
        [video?.id, audio?.id].compactMap { $0 }
    }
}
