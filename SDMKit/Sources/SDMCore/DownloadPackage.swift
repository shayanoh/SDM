import Foundation

public struct DownloadPackage: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public var items: [DownloadItem]
    public var priority: Priority
    /// Position within the download list. Lower sorts earlier.
    public var position: Int
    /// An optional line shown under the package name — e.g. a truncated
    /// playlist's "50 of 320 videos". Parent spec §5.4. `nil` for ordinary
    /// packages.
    public var note: String?

    public init(
        id: UUID = UUID(),
        name: String,
        items: [DownloadItem] = [],
        priority: Priority = .normal,
        position: Int = 0,
        note: String? = nil
    ) {
        precondition(!name.isEmpty, "package name must not be empty")
        self.id = id
        self.name = name
        self.items = items
        self.priority = priority
        self.position = position
        self.note = note
    }

    /// An item's own priority when set, otherwise the package's.
    public func effectivePriority(for item: DownloadItem) -> Priority {
        item.priority ?? priority
    }
}
