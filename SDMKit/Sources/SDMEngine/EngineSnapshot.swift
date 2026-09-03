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
    /// Attempts left before this item becomes terminally `.failed`, or `nil`
    /// if it has never failed. Spec §6.4's "manual retry action" needs this
    /// to show the operator how much budget is left before giving up.
    public let remainingAttempts: Int?
    /// Consecutive failed attempts so far, or `nil` once an attempt succeeds
    /// or makes real progress (the engine resets its counter then). Non-`nil`
    /// on a `.queued` item means it is mid-retry, not freshly enqueued.
    public let failedAttemptCount: Int?
    /// The most recent transient failure's message while the item is retrying,
    /// or `nil` when it is not. A terminal `.failed` already carries its
    /// reason in `state`; this is for the retrying-but-not-yet-terminal gap.
    public let lastFailureReason: String?
    /// Seconds left on the backoff hold before the next attempt, or `nil` when
    /// the item is not being held. Lets the UI say "retrying in Ns".
    public let retryHoldSeconds: Int?
    /// True only for a `.completed` item whose destination file is no longer
    /// on disk — moved or deleted outside SDM. Always `false` for any other
    /// state.
    public let fileMissing: Bool
    /// True while the engine is running `ffmpeg` to assemble a muxed item's
    /// downloaded parts. The item's `state` is still `.running`. Parent
    /// spec §7.2.
    public let isAssembling: Bool
    /// What happens once every component completes — `.none` for a plain
    /// download, `.mux` for a video+audio YouTube item. Lets the UI show
    /// "Assembling…" and offer "Retry Mux" on a failed mux.
    public let assembly: Assembly
    /// Each component's part-file name (`clip.f137.mp4`, `clip.f251.webm`).
    /// For a one-component item this is `[filename]`. The delete dialog and
    /// "reveal in Finder" need the real on-disk names, not just the output
    /// filename.
    public let partFilenames: [String]
    /// A one-line media description shown in the details panel — delivery /
    /// resolution / codecs for a resolver-backed item, release tags mined
    /// from the filename for a plain HTTP one. `nil` / empty ⇒ show a dash.
    public let metadata: String?

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
        checkpointFailure: String? = nil,
        remainingAttempts: Int? = nil,
        failedAttemptCount: Int? = nil,
        lastFailureReason: String? = nil,
        retryHoldSeconds: Int? = nil,
        fileMissing: Bool = false,
        isAssembling: Bool = false,
        assembly: Assembly = .none,
        partFilenames: [String] = [],
        metadata: String? = nil
    ) {
        self.metadata = metadata
        self.checkpointFailure = checkpointFailure
        self.remainingAttempts = remainingAttempts
        self.failedAttemptCount = failedAttemptCount
        self.lastFailureReason = lastFailureReason
        self.retryHoldSeconds = retryHoldSeconds
        self.fileMissing = fileMissing
        self.isAssembling = isAssembling
        self.assembly = assembly
        self.partFilenames = partFilenames.isEmpty ? [filename] : partFilenames
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

    /// A `.queued` item that has already failed at least once — waiting on a
    /// backoff hold before its next attempt, not freshly enqueued.
    public var isRetrying: Bool {
        state == .queued && failedAttemptCount != nil
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
    /// Sum of each item's downloaded bytes so far, for the package header's
    /// "x MB / y MB" aggregate. Same derived-not-stored idiom as
    /// `bytesPerSecond`.
    public var completedBytes: Int64 { items.reduce(0) { $0 + $1.completed.totalBytes } }

    /// Spec §9.6's per-row sparkline data, aggregated the same way
    /// `bytesPerSecond` is: summed from the member items, never stored
    /// separately, so it cannot disagree with them. Shorter histories (an
    /// item added mid-run) are aligned to the trailing edge and front-padded
    /// with zero rather than misaligned by index.
    public var bytesPerSecondHistory: [Double] {
        let length = items.map { $0.speedHistory.count }.max() ?? 0
        guard length > 0 else { return [] }
        var summed = [Double](repeating: 0, count: length)
        for item in items {
            let padding = length - item.speedHistory.count
            for (index, value) in item.speedHistory.enumerated() {
                summed[index + padding] += value
            }
        }
        return summed
    }
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
