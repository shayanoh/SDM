import Foundation
import Testing

@testable import SDMCore

@Test func byteRangeLength() {
    #expect(ByteRange(start: 10, end: 25).length == 15)
}

@Test func byteRangeRejectsInvertedBounds() {
    #expect(ByteRange(start: 10, end: 10).length == 0)
}

@Test func insertIntoEmptySet() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    #expect(set.ranges == [ByteRange(start: 0, end: 100)])
    #expect(set.totalBytes == 100)
}

@Test func insertDisjointRangesStaysSorted() {
    var set = RangeSet()
    set.insert(ByteRange(start: 200, end: 300))
    set.insert(ByteRange(start: 0, end: 100))
    #expect(set.ranges == [ByteRange(start: 0, end: 100), ByteRange(start: 200, end: 300)])
    #expect(set.totalBytes == 200)
}

@Test func adjacentRangesCoalesce() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 100, end: 200))
    #expect(set.ranges == [ByteRange(start: 0, end: 200)])
}

@Test func overlappingRangesMerge() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 50, end: 150))
    #expect(set.ranges == [ByteRange(start: 0, end: 150)])
    #expect(set.totalBytes == 150)
}

@Test func insertBridgingTwoRangesMergesAllThree() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 200, end: 300))
    set.insert(ByteRange(start: 90, end: 210))
    #expect(set.ranges == [ByteRange(start: 0, end: 300)])
}

@Test func insertFullyContainedRangeIsNoOp() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 20, end: 30))
    #expect(set.ranges == [ByteRange(start: 0, end: 100)])
    #expect(set.totalBytes == 100)
}

@Test func insertEmptyRangeIsNoOp() {
    var set = RangeSet()
    set.insert(ByteRange(start: 50, end: 50))
    #expect(set.ranges.isEmpty)
}

@Test func containsRespectsHalfOpenBounds() {
    var set = RangeSet()
    set.insert(ByteRange(start: 10, end: 20))
    #expect(set.contains(10))
    #expect(set.contains(19))
    #expect(!set.contains(20))
    #expect(!set.contains(9))
}

@Test func rangeSetRoundTripsThroughCodable() throws {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 200, end: 300))
    let data = try JSONEncoder().encode(set)
    #expect(try JSONDecoder().decode(RangeSet.self, from: data) == set)
}
