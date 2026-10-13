import Foundation

/// Where a component's bytes come from.
///
/// `.resolved` marks a stream a `LinkResolver` produced: when its URL
/// expires mid-download the engine calls `resolver.refresh(sourceURL:
/// DownloadItem.sourceURL, formatID:)` to get a fresh one. Only the
/// `formatID` is stored — the video's identity lives once, in
/// `DownloadItem.sourceURL`. Parent spec §5.1.
public enum ComponentOrigin: Equatable, Sendable, Hashable {
    case http
    case resolved(formatID: String)
    /// A stream with no single `Range`-capable URL (HLS/DASH). The engine
    /// hands `DownloadItem.sourceURL` to yt-dlp as a downloader with this
    /// `-f` selector; the component is always non-resumable. Parent spec
    /// `2026-09-03-multi-site-resolver-design.md` §6.4.
    case wholesale(formatSelector: String)
}

extension ComponentOrigin: Codable {
    private enum Kind: String, Codable { case http, resolved, wholesale }
    private enum CodingKeys: String, CodingKey {
        // `formatID` carries the yt-dlp format id for `.resolved` and the
        // `-f` selector string for `.wholesale`.
        case kind, formatID
        // legacy: pre-slim `.resolved(extractor:videoID:formatID:)`
        case resolved
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        // Current shape: {"kind":"resolved","formatID":"137"} / {"kind":"http"}.
        // An unrecognized `kind` value (forward-compat) falls through to `.http`.
        if let rawKind = try container.decodeIfPresent(String.self, forKey: .kind),
            let kind = Kind(rawValue: rawKind)
        {
            switch kind {
            case .http: self = .http
            case .resolved:
                self = .resolved(formatID: try container.decode(String.self, forKey: .formatID))
            case .wholesale:
                self = .wholesale(
                    formatSelector: try container.decode(String.self, forKey: .formatID))
            }
            return
        }
        // Legacy synthesized shape: {"resolved":{"extractor":…,"videoID":…,
        // "formatID":"137"}}; anything else (incl. {"http":{}}) is `.http`.
        if let nested = try? container.nestedContainer(
            keyedBy: LegacyResolvedKeys.self, forKey: .resolved)
        {
            self = .resolved(formatID: try nested.decode(String.self, forKey: .formatID))
            return
        }
        self = .http
    }

    private enum LegacyResolvedKeys: String, CodingKey { case extractor, videoID, formatID }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .http:
            try container.encode(Kind.http, forKey: .kind)
        case .resolved(let formatID):
            try container.encode(Kind.resolved, forKey: .kind)
            try container.encode(formatID, forKey: .formatID)
        case .wholesale(let formatSelector):
            try container.encode(Kind.wholesale, forKey: .kind)
            try container.encode(formatSelector, forKey: .formatID)
        }
    }
}

/// What has to happen once every component of an item is fully downloaded.
public enum Assembly: String, Equatable, Codable, Sendable {
    /// One component whose container is already final — just rename it.
    case none
    /// Combine the components with `ffmpeg -c copy`.
    case mux
}

public struct ComponentError: Equatable, Codable, Sendable {
    public var message: String
    public init(_ message: String) { self.message = message }
}

/// One separately-downloaded file backing a `DownloadItem`. A generic HTTP
/// download has exactly one; a muxed YouTube download has two (video +
/// audio). Parent spec §5.1.
public struct FileComponent: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var url: URL
    public var partFilename: String
    public var totalBytes: Int64?
    public var completed: RangeSet
    public var validator: String?
    public var origin: ComponentOrigin
    public var isResumable: Bool?
    public var lastError: ComponentError?

    public init(
        id: UUID = UUID(),
        url: URL,
        partFilename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        validator: String? = nil,
        origin: ComponentOrigin = .http,
        isResumable: Bool? = nil,
        lastError: ComponentError? = nil
    ) {
        self.id = id
        self.url = url
        self.partFilename = partFilename
        self.totalBytes = totalBytes
        self.completed = completed
        self.validator = validator
        self.origin = origin
        self.isResumable = isResumable
        self.lastError = lastError
    }

    public var isComplete: Bool {
        guard let totalBytes else { return false }
        return completed.isComplete(total: totalBytes)
    }

    public var fractionCompleted: Double {
        guard let totalBytes, totalBytes > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(totalBytes)
    }
}
