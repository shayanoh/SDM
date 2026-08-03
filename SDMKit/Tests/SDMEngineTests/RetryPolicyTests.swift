import Foundation
import Testing

@testable import SDMEngine

@Test func connectionDropsAreTransient() {
    #expect(RetryPolicy().classify(TransportError.connectionDropped) == .transient)
}

@Test func serverErrorsAreTransient() {
    #expect(RetryPolicy().classify(TransportError.http(status: 503)) == .transient)
    #expect(RetryPolicy().classify(DownloadError.serverError(status: 500)) == .transient)
}

@Test func notFoundIsPermanent() {
    #expect(
        RetryPolicy().classify(DownloadError.serverError(status: 404))
            == .permanent(reason: "HTTP 404")
    )
}

@Test func forbiddenIsTransientBecauseSignedURLsExpire() {
    #expect(RetryPolicy().classify(DownloadError.serverError(status: 403)) == .transient)
}

@Test func unknownSizeIsPermanent() {
    #expect(
        RetryPolicy().classify(DownloadError.unknownSize)
            == .permanent(reason: "Server did not report a size")
    )
}

@Test func delayGrowsExponentially() {
    let policy = RetryPolicy(maxAttempts: 5, baseDelay: .milliseconds(100))
    #expect(policy.delay(forAttempt: 0) < policy.delay(forAttempt: 1))
    #expect(policy.delay(forAttempt: 1) < policy.delay(forAttempt: 2))
}

@Test func delayIsDeterministicForTheSameAttempt() {
    let policy = RetryPolicy(maxAttempts: 5, baseDelay: .milliseconds(100))
    #expect(policy.delay(forAttempt: 3) == policy.delay(forAttempt: 3))
}

@Test func delayIsCappedAtCeiling() {
    let policy = RetryPolicy(maxAttempts: 20, baseDelay: .seconds(1), maxDelay: .seconds(30))
    #expect(policy.delay(forAttempt: 19) <= .seconds(30))
}

@Test func truncatedResponseIsTransient() {
    #expect(
        RetryPolicy().classify(DownloadError.truncatedResponse(expected: 1000, received: 500))
            == .transient
    )
}
