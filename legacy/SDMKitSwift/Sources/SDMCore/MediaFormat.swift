import Foundation

public enum MediaKind: String, Sendable, Codable, Equatable {
    case progressive, videoOnly, audioOnly
}

/// How a format's bytes are delivered. `.direct` is a single
/// `Range`-capable http(s) URL the engine downloads itself; `.hls` / `.dash`
/// are segmented manifests with no single URL, handed to yt-dlp wholesale.
public enum MediaDelivery: String, Sendable, Codable, Equatable {
    case direct, hls, dash
}

public enum VideoCodec: Sendable, Codable, Equatable, Hashable {
    case av1, vp9, h264
    case other(String)

    public static let priority: [VideoCodec] = [.av1, .vp9, .h264]

    public var rank: Int {
        VideoCodec.priority.firstIndex(of: self) ?? Int.max
    }
}

public enum AudioCodec: Sendable, Codable, Equatable, Hashable {
    case opus, aac
    case other(String)

    public static let priority: [AudioCodec] = [.opus, .aac]

    public var rank: Int {
        AudioCodec.priority.firstIndex(of: self) ?? Int.max
    }
}

public enum MediaContainer: Sendable, Codable, Equatable, Hashable {
    case mp4, webm, m4a
    case other(String)

    public static let priority: [MediaContainer] = [.mp4, .webm]

    public var rank: Int {
        MediaContainer.priority.firstIndex(of: self) ?? Int.max
    }

    public var fileExtension: String {
        switch self {
        case .mp4: return "mp4"
        case .webm: return "webm"
        case .m4a: return "m4a"
        case .other(let ext): return ext
        }
    }

    public static func fromFileExtension(_ ext: String) -> MediaContainer {
        switch ext.lowercased() {
        case "mp4", "m4v": return .mp4
        case "webm": return .webm
        case "m4a": return .m4a
        case "": return .other("bin")
        default: return .other(ext.lowercased())
        }
    }
}

public struct MediaFormat: Sendable, Codable, Equatable, Identifiable {
    public let id: String
    public var kind: MediaKind
    public var height: Int?
    public var width: Int?
    public var vcodec: VideoCodec?
    public var acodec: AudioCodec?
    public var container: MediaContainer
    public var filesize: Int64?
    public var filesizeApprox: Int64?
    public var tbr: Double?
    public var url: URL
    public var delivery: MediaDelivery

    public init(
        id: String, kind: MediaKind, height: Int?, width: Int?,
        vcodec: VideoCodec?, acodec: AudioCodec?, container: MediaContainer,
        filesize: Int64?, filesizeApprox: Int64?, tbr: Double?, url: URL,
        delivery: MediaDelivery = .direct
    ) {
        self.id = id
        self.kind = kind
        self.height = height
        self.width = width
        self.vcodec = vcodec
        self.acodec = acodec
        self.container = container
        self.filesize = filesize
        self.filesizeApprox = filesizeApprox
        self.tbr = tbr
        self.url = url
        self.delivery = delivery
    }

    public var filesizeEffective: Int64? { filesize ?? filesizeApprox }
    public var isApproximateSize: Bool { filesize == nil }
    /// A single `Range`-capable URL the engine downloads itself.
    public var isDirect: Bool { delivery == .direct }
}
