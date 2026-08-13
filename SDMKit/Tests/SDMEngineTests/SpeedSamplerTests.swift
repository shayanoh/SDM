import SDMCore
import Testing

@testable import SDMEngine

@Test func newSamplerReportsZero() {
    #expect(SpeedSampler().bytesPerSecond == 0)
}

@Test func firstTickReportsBytesRecordedInThatTickScaledToOneSecond() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func recordAccumulatesWithinOneTick() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 400)
    sampler.record(bytes: 600)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func tickScalesAPartialSecondWindowUpToBytesPerSecond() {
    // At 5 ticks/second each tick covers 0.2 s, so 200 bytes in one tick is
    // 1000 bytes/s.
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 5)
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func bytesPerSecondAveragesOnlyTheTrailingWindow() {
    // A 2-second window at 1 tick/second is 2 samples: the third tick's
    // 3000 must push the first tick's 1000 out of the average.
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
    sampler.record(bytes: 3000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 2000)
}

@Test func idleReportsZeroImmediatelyRatherThanDecaying() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1_000_000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1_000_000)
    sampler.idle()
    #expect(sampler.bytesPerSecond == 0)
}

@Test func tickAfterIdleResumesReportingImmediately() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    sampler.idle()
    #expect(sampler.bytesPerSecond == 0)
    sampler.record(bytes: 500)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 500)
}

@Test func historyRecordsRawPerSecondEstimatesInOrder() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.history == [100, 200])
}

@Test func idleAppendsAZeroSampleToHistory() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.idle()
    #expect(sampler.history == [100, 0])
}

/// Regression: a heartbeat tick keeps calling `idle()` on every sampler for
/// every non-running item, every tick, indefinitely — the caller has no way
/// to know it already went idle and stop calling. Without this, `history`
/// (and therefore the whole `SpeedSampler` value) kept changing shape for a
/// full `historyLength` worth of ticks after any item went idle, purely from
/// harmless-looking repeated `idle()` calls, which defeated equality-based
/// change detection one layer up (`EngineController` comparing telemetry
/// snapshots to decide whether to republish).
@Test func idleCalledAgainWhileAlreadyIdleDoesNotMutateHistoryFurther() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.idle()
    let afterFirstIdle = sampler
    sampler.idle()
    sampler.idle()
    sampler.idle()
    #expect(sampler.history == afterFirstIdle.history)
    #expect(sampler.history == [100, 0])
}

@Test func historyIsCappedAtItsLength() {
    var sampler = SpeedSampler(historyLength: 3, averagingWindowSeconds: 2, ticksPerSecond: 1)
    for value in 1...5 {
        sampler.record(bytes: Int64(value * 100))
        sampler.tick()
    }
    #expect(sampler.history == [300, 400, 500])
}

@Test func defaultHistoryLengthScalesWithAppTiming() {
    var sampler = SpeedSampler()
    for value in 1...(AppTiming.ticksPerSecond * 60 * 10 + 5) {
        sampler.record(bytes: Int64(value))
        sampler.tick()
    }
    #expect(sampler.history.count == AppTiming.ticksPerSecond * 60 * 10)
}
