public enum Priority: Int, Comparable, Codable, CaseIterable, Sendable {
    case lowest = 0
    case low = 1
    case normal = 2
    case high = 3
    case highest = 4

    public static func < (lhs: Priority, rhs: Priority) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

/// Lifecycle state of a download.
///
/// There is deliberately no `paused` or `disabled` case: pausing is
/// `isEnabled == false` on the item, and a preempted item returns to `queued`.
public enum ItemState: Equatable, Codable, Sendable {
    case queued
    case running
    case completed
    case failed(reason: String)
}
