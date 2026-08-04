import Foundation
import SDMCore

/// An immutable view of engine state, published to the UI. Views consume these
/// rather than touching engine actors. See spec §5.4 and §9.6.
public struct ItemSnapshot: Sendable, Equatable, Identifiable {
    public let id: UUID
    public let url: URL
    public let filename: String
    public let totalBytes: Int64?
    public let completed: RangeSet
    public let state: ItemState
    public let isEnabled: Bool
    /// Three-state, mirroring `DownloadItem.isResumable`: `nil` means the
    /// origin has not been probed yet. Not flattened, because the UI badge
    /// has to distinguish "cannot be resumed" from "don't know yet" — they
    /// warrant different affordances, and only the first is a warning.
    public let isResumable: Bool?
    public let activeSegments: Int
    public let configuredSegments: Int
    public let bytesPerSecond: Double
    public let speedHistory: [Double]
    /// Why this download's resume state could not be written, or `nil` when
    /// checkpointing is healthy. Surfaced because a sidecar that silently
    /// fails to write means a crash loses all progress with no signal — the
    /// user needs to know the download is not actually resumable right now.
    public let checkpointFailure: String?

    public init(
        id: UUID,
        url: URL,
        filename: String,
        totalBytes: Int64?,
        completed: RangeSet,
        state: ItemState,
        isEnabled: Bool,
        isResumable: Bool?,
        activeSegments: Int,
        configuredSegments: Int,
        bytesPerSecond: Double,
        speedHistory: [Double],
        checkpointFailure: String? = nil
    ) {
        self.checkpointFailure = checkpointFailure
        self.id = id
        self.url = url
        self.filename = filename
        self.totalBytes = totalBytes
        self.completed = completed
        self.state = state
        self.isEnabled = isEnabled
        self.isResumable = isResumable
        self.activeSegments = activeSegments
        self.configuredSegments = configuredSegments
        self.bytesPerSecond = bytesPerSecond
        self.speedHistory = speedHistory
    }

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(total)
    }
}

public struct PackageSnapshot: Sendable, Equatable, Identifiable {
    public let id: UUID
    public let name: String
    public let priority: Priority
    public let items: [ItemSnapshot]

    public init(id: UUID, name: String, priority: Priority, items: [ItemSnapshot]) {
        self.id = id
        self.name = name
        self.priority = priority
        self.items = items
    }

    /// Spec §5.4: aggregates are computed from the item figures, never stored
    /// alongside them, so package and global speed cannot disagree with the
    /// per-item numbers they are made of.
    public var bytesPerSecond: Double { items.reduce(0) { $0 + $1.bytesPerSecond } }
    public var completedCount: Int { items.filter { $0.state == .completed }.count }
    public var totalBytes: Int64 { items.reduce(0) { $0 + ($1.totalBytes ?? 0) } }
}

public struct EngineSnapshot: Sendable, Equatable {
    public let packages: [PackageSnapshot]
    public let globalBytesPerSecond: Double
    public let globalHistory: [Double]

    /// Public so the app target can construct an empty starting value.
    public init(
        packages: [PackageSnapshot],
        globalBytesPerSecond: Double,
        globalHistory: [Double]
    ) {
        self.packages = packages
        self.globalBytesPerSecond = globalBytesPerSecond
        self.globalHistory = globalHistory
    }
}
