import Testing

@testable import SDMCore

@Test func gapsOfEmptySetIsWholeFile() {
    #expect(RangeSet().gaps(within: 100) == [ByteRange(start: 0, end: 100)])
}

@Test func gapsBetweenAndAfterRanges() {
    let set = RangeSet([ByteRange(start: 10, end: 20), ByteRange(start: 40, end: 50)])
    #expect(
        set.gaps(within: 100) == [
            ByteRange(start: 0, end: 10),
            ByteRange(start: 20, end: 40),
            ByteRange(start: 50, end: 100),
        ]
    )
}

@Test func noGapsWhenComplete() {
    let set = RangeSet([ByteRange(start: 0, end: 100)])
    #expect(set.gaps(within: 100).isEmpty)
    #expect(set.isComplete(total: 100))
}

@Test func incompleteWhenAnyGapRemains() {
    let set = RangeSet([ByteRange(start: 0, end: 99)])
    #expect(!set.isComplete(total: 100))
}

@Test func firstClaimTakesFirstHalfOfWholeFile() {
    let claim = RangeSet().nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 0, end: 500))
}

@Test func secondClaimAvoidsReservedRange() {
    let claim = RangeSet().nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 0, end: 500)],
        minChunk: 10
    )
    #expect(claim == ByteRange(start: 500, end: 750))
}

@Test func smallGapIsTakenWhole() {
    let claim = RangeSet().nextClaim(total: 15, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 0, end: 15))
}

@Test func claimPrefersLargestGap() {
    let set = RangeSet([ByteRange(start: 100, end: 200)])
    let claim = set.nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 200, end: 600))
}

@Test func noClaimWhenEverythingIsDoneOrReserved() {
    let set = RangeSet([ByteRange(start: 0, end: 500)])
    let claim = set.nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 500, end: 1000)],
        minChunk: 10
    )
    #expect(claim == nil)
}

@Test func claimSkipsFragmentedHolesLeftByRetiredWorkers() {
    let set = RangeSet([
        ByteRange(start: 0, end: 100),
        ByteRange(start: 150, end: 400),
        ByteRange(start: 450, end: 1000),
    ])
    let claim = set.nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 100, end: 125))
}
