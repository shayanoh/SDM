import Foundation

/// One link's progress through the two-stage probing pipeline. Spec §7.5:
/// rows appear immediately as `.queued` and fill in as probes land.
public struct ProbedLink: Identifiable, Equatable, Sendable {
    public enum Stage: Equatable, Sendable {
        case queued
        case probing
        case sniffing
        case done
    }

    public let id: UUID
    public let originalURL: URL
    public var finalURL: URL
    public var stage: Stage
    public var statusCode: Int?
    public var contentLength: Int64?
    public var contentType: String?
    public var suggestedFilename: String?
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    public var validator: String?
    /// Whether a `206` came back — spec §7.2's resume-capable flag.
    public var acceptsRanges: Bool
    public var sniffedSignature: FileSignature?
    /// True when both the HEAD and the ranged-GET fallback threw — a DNS or
    /// timeout failure rather than an HTTP-level error status.
    public var transportFailed: Bool
    public var verdict: Verdict?
    public var isDuplicate: Bool

    public init(
        id: UUID = UUID(),
        originalURL: URL,
        finalURL: URL? = nil,
        stage: Stage = .queued,
        statusCode: Int? = nil,
        contentLength: Int64? = nil,
        contentType: String? = nil,
        suggestedFilename: String? = nil,
        validator: String? = nil,
        acceptsRanges: Bool = false,
        sniffedSignature: FileSignature? = nil,
        transportFailed: Bool = false,
        verdict: Verdict? = nil,
        isDuplicate: Bool = false
    ) {
        self.id = id
        self.originalURL = originalURL
        self.finalURL = finalURL ?? originalURL
        self.stage = stage
        self.statusCode = statusCode
        self.contentLength = contentLength
        self.contentType = contentType
        self.suggestedFilename = suggestedFilename
        self.validator = validator
        self.acceptsRanges = acceptsRanges
        self.sniffedSignature = sniffedSignature
        self.transportFailed = transportFailed
        self.verdict = verdict
        self.isDuplicate = isDuplicate
    }

    /// The name used for clustering and display: the server-declared name
    /// when Stage 1 captured one, otherwise the last URL path component.
    public var effectiveFilename: String {
        if let suggestedFilename, !suggestedFilename.isEmpty { return suggestedFilename }
        let last = finalURL.lastPathComponent
        return last.isEmpty ? "download" : last
    }
}
