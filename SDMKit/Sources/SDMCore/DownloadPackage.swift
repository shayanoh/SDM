import Foundation

public struct DownloadPackage: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public var items: [DownloadItem]
    public var priority: Priority
    /// Position within the download list. Lower sorts earlier.
    public var position: Int

    public init(
        id: UUID = UUID(),
        name: String,
        items: [DownloadItem] = [],
        priority: Priority = .normal,
        position: Int = 0
    ) {
        precondition(!name.isEmpty, "package name must not be empty")
        self.id = id
        self.name = name
        self.items = items
        self.priority = priority
        self.position = position
    }

    /// An item's own priority when set, otherwise the package's.
    public func effectivePriority(for item: DownloadItem) -> Priority {
        item.priority ?? priority
    }
}
