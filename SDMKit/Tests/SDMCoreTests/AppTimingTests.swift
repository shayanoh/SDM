import Testing

@testable import SDMCore

@Test func ticksPerSecondIsPositive() {
    #expect(AppTiming.ticksPerSecond > 0)
}

@Test func ticksPerSecondDefaultsToFive() {
    #expect(AppTiming.ticksPerSecond == 5)
}
