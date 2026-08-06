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
/// Two independent axes, on purpose (a reversal of this codebase's earlier
/// single-axis design): `state` is the scheduling axis — `stopped` means
/// "not desired right now," `queued` means "eligible, waiting for a slot,"
/// and a preempted item returns to `queued`, never `stopped`. `isEnabled` on
/// `DownloadItem` is the orthogonal, purely user-managed axis — "never start
/// this no matter what" — and nothing but an explicit user action may ever
/// change it. Pause All / Resume All only ever touch `state`; Disable /
/// Enable only ever touch `isEnabled`.
public enum ItemState: Equatable, Codable, Sendable {
    case stopped
    case queued
    case running
    case completed
    case failed(reason: String)
}
