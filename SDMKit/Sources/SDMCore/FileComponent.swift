import Foundation

/// Where a component's bytes come from, and what is needed to refresh its
/// URL if it expires mid-download. Parent spec §5.1.
public enum ComponentOrigin: Equatable, Codable, Sendable, Hashable {
    case http
    case resolved(extractor: String, videoID: String, formatID: String)
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
        precondition(!partFilename.isEmpty, "partFilename must not be empty")
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
