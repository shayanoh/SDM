import Foundation
import SDMCore

public enum MediaRowState: Sendable, Equatable {
    case resolving
    case resolved
    case unselected
    case unsupported
    case needsYtDlp
    case needsFfmpeg
    case failed(String)
}

/// One YouTube (or other extractor-backed) URL in the linkgrabber — the
/// media counterpart of `ProbedLink`. Parent spec §6.2.
public struct MediaRow: Identifiable, Sendable, Equatable {
    public let id: UUID
    public let sourceURL: URL
    public var title: String
    public var state: MediaRowState
    public var media: ResolvedMedia?
    public var choice: FormatChoice?
    public var playlistGroup: UUID?
    public var isDuplicate: Bool

    public init(
        id: UUID = UUID(),
        sourceURL: URL,
        title: String? = nil,
        state: MediaRowState = .resolving,
        media: ResolvedMedia? = nil,
        choice: FormatChoice? = nil,
        playlistGroup: UUID? = nil,
        isDuplicate: Bool = false
    ) {
        self.id = id
        self.sourceURL = sourceURL
        let last = sourceURL.lastPathComponent
        self.title = title ?? (last.isEmpty ? "video" : last)
        self.state = state
        self.media = media
        self.choice = choice
        self.playlistGroup = playlistGroup
        self.isDuplicate = isDuplicate
    }

    /// `"<sanitized title> [<videoID>].<container>"` once resolved with a
    /// chosen format; the URL-derived title before that. Used for clustering
    /// and as the eventual `DownloadItem.outputFilename`.
    public var displayFilename: String {
        guard let media, choice != nil else { return title }
        let container = choice?.outputContainer ?? .mp4
        return "\(Self.sanitize(media.title)) [\(media.videoID)].\(container.fileExtension)"
    }

    public var combinedBytes: Int64? { choice?.estimatedBytes }

    static func sanitize(_ raw: String) -> String {
        let kept = raw.unicodeScalars.filter {
            $0 != "/" && !CharacterSet.controlCharacters.contains($0)
        }
        let collapsed = String(String.UnicodeScalarView(kept))
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
        return collapsed.isEmpty ? "video" : String(collapsed.prefix(180))
    }
}
