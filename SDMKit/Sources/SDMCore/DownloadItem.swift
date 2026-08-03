import Foundation

public struct DownloadItem: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var url: URL
    public var filename: String
    public var totalBytes: Int64?
    public var completed: RangeSet
    public var state: ItemState
    public var isEnabled: Bool
    /// Whether the origin honors `Range` requests, or `nil` when it has not
    /// been probed yet.
    ///
    /// Three-state on purpose. Only the engine can answer this, and only after
    /// a probe, so a freshly grabbed item genuinely does not know. Collapsing
    /// "unknown" into `false` would make every new item look non-resumable,
    /// and spec §6.3 gives running non-resumable items an unconditional claim
    /// on their slot — which would make nothing in the list preemptible.
    public var isResumable: Bool?
    public var priority: Priority?
    /// Position within the owning package. Lower sorts earlier.
    public var position: Int
    /// Server validator captured at download start, used to detect a changed remote file.
    public var validator: String?

    public init(
        id: UUID = UUID(),
        url: URL,
        filename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        state: ItemState = .queued,
        isEnabled: Bool = true,
        isResumable: Bool? = nil,
        priority: Priority? = nil,
        position: Int = 0,
        validator: String? = nil
    ) {
        precondition(!filename.isEmpty, "filename must not be empty")
        self.id = id
        self.url = url
        self.filename = filename
        self.totalBytes = totalBytes
        self.completed = completed
        self.state = state
        self.isEnabled = isEnabled
        self.isResumable = isResumable
        self.priority = priority
        self.position = position
        self.validator = validator
    }

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(total)
    }

    public var isComplete: Bool {
        guard let total = totalBytes else { return false }
        return completed.isComplete(total: total)
    }
}
