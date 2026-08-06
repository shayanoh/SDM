import Foundation

public struct ConnectionBudget: Sendable {
    public var global: Int
    public var perHost: Int

    public init(global: Int, perHost: Int) {
        precondition(global >= 1, "global must be at least 1")
        precondition(perHost >= 1, "perHost must be at least 1")
        self.global = global
        self.perHost = perHost
    }
}

public struct ConnectionDemand: Sendable {
    public let id: UUID
    public let host: String
    public let desiredSegments: Int

    public init(id: UUID, host: String, desiredSegments: Int) {
        precondition(desiredSegments >= 1, "desiredSegments must be at least 1")
        self.id = id
        self.host = host
        self.desiredSegments = desiredSegments
    }
}

/// Shrinks per-item worker-pool sizes to fit within a global connection
/// ceiling and a per-host ceiling. Spec §6.4: "worker pools shrink below
/// their configured N when the budget is tight, largest pool yielding
/// first."
///
/// Never shrinks a pool below 1: if every pool is already at the floor and
/// the total still exceeds `budget.global`, the ceiling is a soft target
/// from here on — killing an item outright to enforce it is the scheduler's
/// job (`maxConcurrent`), not this one's.
public enum ConnectionAllocator {
    public static func allocate(
        demands: [ConnectionDemand],
        budget: ConnectionBudget
    ) -> [UUID: Int] {
        var allocated: [UUID: Int] = [:]
        for demand in demands { allocated[demand.id] = demand.desiredSegments }

        func totalForHost(_ host: String) -> Int {
            demands.filter { $0.host == host }.reduce(0) { $0 + (allocated[$1.id] ?? 0) }
        }
        func shrinkLargest(among candidates: [ConnectionDemand]) -> Bool {
            guard
                let victim =
                    candidates
                    .filter({ (allocated[$0.id] ?? 0) > 1 })
                    .max(by: { (allocated[$0.id] ?? 0) < (allocated[$1.id] ?? 0) })
            else { return false }
            allocated[victim.id] = (allocated[victim.id] ?? 1) - 1
            return true
        }

        // Per-host pass first: each over-budget host is brought into line on
        // its own, independent of every other host.
        for host in Set(demands.map(\.host)) {
            let hostDemands = demands.filter { $0.host == host }
            while totalForHost(host) > budget.perHost {
                guard shrinkLargest(among: hostDemands) else { break }
            }
        }

        // Global pass: shrinks the largest remaining pool across every host
        // until the grand total fits.
        while allocated.values.reduce(0, +) > budget.global {
            guard shrinkLargest(among: demands) else { break }
        }

        return allocated
    }
}
