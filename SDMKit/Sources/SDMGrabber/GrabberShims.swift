import Foundation

extension ProbedLink {
    public var originalURL: URL { originalUrl }
    public var finalURL: URL { finalUrl }
    /// The name used for clustering and display (rule lives in Rust).
    public var effectiveFilename: String {
        (try? probedLinkEffectiveFilename(link: self)) ?? "download"
    }
}

extension MediaRow {
    public var sourceURL: URL { sourceUrl }
    public var combinedBytes: Int64? { choice?.estimatedBytes }
    public var displayFilename: String {
        (try? mediaRowDisplayFilename(row: self)) ?? title
    }
}

extension PackageCandidate {
    public var linkIDs: [UUID] { linkIds }
}

extension GrabberSnapshot {
    /// What the app shows before the first ingest.
    public static let empty = GrabberSnapshot(
        links: [], mediaRows: [], packages: [], checkedCount: 0, totalCount: 0)

    /// The verdict counts, computed in Rust. All zero if the call fails.
    public var summary: GrabberSummary {
        (try? grabberSnapshotSummary(snapshot: self))
            ?? GrabberSummary(
                onlineCount: 0, offlineCount: 0, faultyCount: 0, failedCount: 0,
                isChecking: false, recheckableCount: 0)
    }
    public var onlineCount: Int { summary.onlineCount }
    public var offlineCount: Int { summary.offlineCount }
    public var faultyCount: Int { summary.faultyCount }
    public var failedCount: Int { summary.failedCount }
    public var isChecking: Bool { summary.isChecking }
    public var recheckableCount: Int { summary.recheckableCount }
}

/// The old namespaces, forwarding to the exported functions.
public enum MediaHandoff {
    public static func build(httpLinks: [ProbedLink], mediaRows: [MediaRow]) -> (
        items: [DownloadItem], heldBackCount: Int
    ) {
        guard let result = try? mediaHandoffBuild(httpLinks: httpLinks, mediaRows: mediaRows)
        else { return ([], httpLinks.count + mediaRows.count) }
        return (result.items, result.heldBackCount)
    }
}

public enum MediaFormatMenu {
    public static func options(for media: ResolvedMedia, preferences: QualityPreferences)
        -> [MediaFormatOption]
    {
        (try? mediaFormatMenuOptions(media: media, preferences: preferences)) ?? []
    }
}

public enum URLExtractor {
    public static func extractLinks(from text: String) -> [URL] {
        (try? extractUrls(text: text)) ?? []
    }
}
