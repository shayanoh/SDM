/// An immutable view of grabber state, published to the UI. Spec §7.5's
/// determinate `checked / total` header and verdict filter chips read
/// straight off this.
public struct GrabberSnapshot: Sendable, Equatable {
    public let links: [ProbedLink]
    public let mediaRows: [MediaRow]
    public let packages: [PackageCandidate]
    public let checkedCount: Int
    public let totalCount: Int

    public init(
        links: [ProbedLink],
        mediaRows: [MediaRow] = [],
        packages: [PackageCandidate],
        checkedCount: Int,
        totalCount: Int
    ) {
        self.links = links
        self.mediaRows = mediaRows
        self.packages = packages
        self.checkedCount = checkedCount
        self.totalCount = totalCount
    }

    public var onlineCount: Int { links.filter { $0.verdict == .online }.count }
    public var offlineCount: Int { links.filter { $0.verdict == .offline }.count }
    public var faultyCount: Int {
        links.filter {
            if case .faulty = $0.verdict { return true }
            return false
        }.count
    }
    public var failedCount: Int { links.filter { $0.verdict == .checkFailed }.count }

    /// Rows a "Recheck All" would actually retry: failed HTTP probes plus
    /// media rows that failed to resolve or need a binary.
    public var recheckableCount: Int {
        failedCount
            + mediaRows.filter {
                switch $0.state {
                case .failed, .needsYtDlp, .needsFfmpeg: return true
                default: return false
                }
            }.count
    }
}
