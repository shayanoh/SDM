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

@Test func firstClaimTakesTheWholeFile() {
    // No pre-emptive halving: a fresh claim takes the entire free gap.
    // Splitting only happens later, on demand, when another worker needs
    // work and none is free — see `DownloadTask`'s stealing logic.
    let claim = RangeSet().nextClaim(total: 1000, reserved: [])
    #expect(claim == ByteRange(start: 0, end: 1000))
}

@Test func secondClaimTakesTheWholeRemainderPastAReservedRange() {
    let claim = RangeSet().nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 0, end: 500)]
    )
    #expect(claim == ByteRange(start: 500, end: 1000))
}

@Test func smallGapIsTakenWhole() {
    let claim = RangeSet().nextClaim(total: 15, reserved: [])
    #expect(claim == ByteRange(start: 0, end: 15))
}

@Test func claimPrefersLargestGap() {
    let set = RangeSet([ByteRange(start: 100, end: 200)])
    let claim = set.nextClaim(total: 1000, reserved: [])
    #expect(claim == ByteRange(start: 200, end: 1000))
}

@Test func noClaimWhenEverythingIsDoneOrReserved() {
    let set = RangeSet([ByteRange(start: 0, end: 500)])
    let claim = set.nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 500, end: 1000)]
    )
    #expect(claim == nil)
}

@Test func claimSkipsFragmentedHolesLeftByRetiredWorkersPreferringTheLowerStartOnATie() {
    let set = RangeSet([
        ByteRange(start: 0, end: 100),
        ByteRange(start: 150, end: 400),
        ByteRange(start: 450, end: 1000),
    ])
    let claim = set.nextClaim(total: 1000, reserved: [])
    #expect(claim == ByteRange(start: 100, end: 150))
}
