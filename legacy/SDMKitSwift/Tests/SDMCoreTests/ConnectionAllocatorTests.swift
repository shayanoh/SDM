import Foundation
import Testing

@testable import SDMCore

@Test func allocateWithinBudgetReturnsRequestedCounts() {
    let a = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 4)
    let b = ConnectionDemand(id: UUID(), host: "b.com", desiredSegments: 4)
    let allocated = ConnectionAllocator.allocate(
        demands: [a, b],
        budget: ConnectionBudget(global: 32, perHost: 8)
    )
    #expect(allocated[a.id] == 4)
    #expect(allocated[b.id] == 4)
}

@Test func allocateShrinksTheLargestPoolFirstUnderTheGlobalCap() {
    let small = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 2)
    let large = ConnectionDemand(id: UUID(), host: "b.com", desiredSegments: 8)
    let allocated = ConnectionAllocator.allocate(
        demands: [small, large],
        budget: ConnectionBudget(global: 6, perHost: 8)
    )
    #expect(allocated[small.id] == 2)
    #expect(allocated[large.id] == 4)
}

@Test func allocateAppliesThePerHostCapIndependentlyOfTheGlobalCap() {
    let demand = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 8)
    let allocated = ConnectionAllocator.allocate(
        demands: [demand],
        budget: ConnectionBudget(global: 32, perHost: 3)
    )
    #expect(allocated[demand.id] == 3)
}

@Test func allocateNeverShrinksAPoolBelowOne() {
    let demands = (0..<5).map { _ in ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 1)
    }
    let allocated = ConnectionAllocator.allocate(
        demands: demands,
        budget: ConnectionBudget(global: 2, perHost: 8)
    )
    #expect(demands.allSatisfy { (allocated[$0.id] ?? 0) == 1 })
}

@Test func allocateShrinksOnlyTheOverBudgetHostLeavingOthersUntouched() {
    let busy = ConnectionDemand(id: UUID(), host: "busy.com", desiredSegments: 10)
    let quiet = ConnectionDemand(id: UUID(), host: "quiet.com", desiredSegments: 4)
    let allocated = ConnectionAllocator.allocate(
        demands: [busy, quiet],
        budget: ConnectionBudget(global: 32, perHost: 5)
    )
    #expect(allocated[busy.id] == 5)
    #expect(allocated[quiet.id] == 4)
}

@Test func allocateOfEmptyDemandsIsEmpty() {
    #expect(
        ConnectionAllocator.allocate(demands: [], budget: ConnectionBudget(global: 8, perHost: 4))
            .isEmpty)
}
