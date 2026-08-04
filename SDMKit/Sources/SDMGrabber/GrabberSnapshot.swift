/// An immutable view of grabber state, published to the UI. Spec §7.5's
/// determinate `checked / total` header and verdict filter chips read
/// straight off this.
public struct GrabberSnapshot: Sendable, Equatable {
    public let links: [ProbedLink]
    public let packages: [PackageCandidate]
    public let checkedCount: Int
    public let totalCount: Int

    public init(
        links: [ProbedLink],
        packages: [PackageCandidate],
        checkedCount: Int,
        totalCount: Int
    ) {
        self.links = links
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
}
