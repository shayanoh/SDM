import Testing

@testable import SDMEngine

@Test func newSamplerReportsZero() {
    #expect(SpeedSampler().bytesPerSecond == 0)
}

@Test func firstTickReportsBytesRecordedInThatSecond() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func recordAccumulatesWithinOneTick() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 400)
    sampler.record(bytes: 600)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func counterResetsAfterEachTick() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 1000)
    sampler.tick()
    sampler.tick()
    #expect(sampler.bytesPerSecond == 0)
}

@Test func smoothingDampensSuddenChanges() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 0.5)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 500)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 750)
}

@Test func historyRecordsRawSamplesInOrder() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.history == [100, 200])
}

@Test func historyIsCappedAtItsLength() {
    var sampler = SpeedSampler(historyLength: 3, smoothingFactor: 1.0)
    for value in 1...5 {
        sampler.record(bytes: Int64(value * 100))
        sampler.tick()
    }
    #expect(sampler.history == [300, 400, 500])
}

@Test func runningAverageIgnoresEmptyHistory() {
    #expect(SpeedSampler().runningAverage == 0)
}

@Test func runningAverageIsTheMeanOfHistory() {
    var sampler = SpeedSampler(historyLength: 10, smoothingFactor: 1.0)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 300)
    sampler.tick()
    #expect(sampler.runningAverage == 200)
}
