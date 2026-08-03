import Foundation
import SDMCore

public struct SchedulerInput: Sendable {
    public var packages: [DownloadPackage]
    /// Items the engine currently has running.
    public var runningNow: Set<UUID>
    /// Items started within the hysteresis window; protected from preemption.
    public var startedRecently: Set<UUID>
    public var maxConcurrent: Int

    public init(
        packages: [DownloadPackage],
        runningNow: Set<UUID>,
        startedRecently: Set<UUID>,
        maxConcurrent: Int
    ) {
        precondition(maxConcurrent >= 0, "maxConcurrent must be non-negative")
        self.packages = packages
        self.runningNow = runningNow
        self.startedRecently = startedRecently
        self.maxConcurrent = maxConcurrent
    }
}

/// The scheduling policy, expressed as a pure function so every behavior in
/// spec §6 is table-testable. Re-evaluated on every change rather than
/// maintained as a queue.
public enum Scheduler {
    /// Items eligible to run, ordered best-first.
    ///
    /// Sort key: effective priority descending, then package position, then
    /// item position.
    public static func rank(_ packages: [DownloadPackage]) -> [DownloadItem] {
        var scored: [(item: DownloadItem, priority: Priority, packagePosition: Int)] = []
        for package in packages {
            for item in package.items where isEligible(item) {
                scored.append((item, package.effectivePriority(for: item), package.position))
            }
        }
        return
            scored
            .sorted { lhs, rhs in
                if lhs.priority != rhs.priority { return lhs.priority > rhs.priority }
                if lhs.packagePosition != rhs.packagePosition {
                    return lhs.packagePosition < rhs.packagePosition
                }
                return lhs.item.position < rhs.item.position
            }
            .map(\.item)
    }

    /// The set of item IDs that should be running right now.
    ///
    /// Slots are allocated in three passes: running non-resumable items (which
    /// cannot be preempted), then running items still inside the hysteresis
    /// window, then the highest-ranked remainder.
    public static func desiredRunningSet(_ input: SchedulerInput) -> Set<UUID> {
        let ranked = rank(input.packages)
        let eligibleIDs = Set(ranked.map(\.id))
        var slots = input.maxConcurrent
        var desired: Set<UUID> = []

        func reserve(_ candidates: [DownloadItem]) {
            for candidate in candidates where slots > 0 && !desired.contains(candidate.id) {
                desired.insert(candidate.id)
                slots -= 1
            }
        }

        // Pass 1: running non-resumable items keep their slots unconditionally.
        reserve(
            ranked.filter {
                input.runningNow.contains($0.id) && !$0.isResumable
            }
        )

        // Pass 2: recently started items are protected from drag-induced churn.
        reserve(
            ranked.filter {
                input.runningNow.contains($0.id) && input.startedRecently.contains($0.id)
            }
        )

        // Pass 3: fill whatever remains by rank.
        reserve(ranked)

        return desired.intersection(eligibleIDs)
    }

    private static func isEligible(_ item: DownloadItem) -> Bool {
        guard item.isEnabled else { return false }
        switch item.state {
        case .queued, .running: return true
        case .completed, .failed: return false
        }
    }
}
