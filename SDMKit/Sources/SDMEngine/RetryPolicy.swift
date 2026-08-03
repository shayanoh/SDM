import Foundation

public enum FailureKind: Equatable, Sendable {
    case transient
    case permanent(reason: String)
}

public struct RetryPolicy: Sendable {
    public var maxAttempts: Int
    public var baseDelay: Duration
    public var maxDelay: Duration

    public init(
        maxAttempts: Int = 5,
        baseDelay: Duration = .seconds(1),
        maxDelay: Duration = .seconds(60)
    ) {
        precondition(maxAttempts >= 1, "maxAttempts must be at least 1")
        self.maxAttempts = maxAttempts
        self.baseDelay = baseDelay
        self.maxDelay = maxDelay
    }

    public func classify(_ error: any Error) -> FailureKind {
        if let transport = error as? TransportError {
            switch transport {
            case .connectionDropped, .malformedResponse:
                return .transient
            case .http(let status):
                return Self.classify(status: status)
            }
        }
        if let download = error as? DownloadError {
            switch download {
            case .serverError(let status):
                return Self.classify(status: status)
            case .unknownSize:
                return .permanent(reason: "Server did not report a size")
            case .incompleteAfterWorkersFinished:
                return .transient
            case .truncatedResponse:
                return .transient
            }
        }
        return .transient
    }

    /// 403 is transient on purpose: signed media URLs (googlevideo and
    /// friends) expire, and the correct response is to refresh and retry.
    private static func classify(status: Int) -> FailureKind {
        switch status {
        case 403, 408, 425, 429, 500...599:
            return .transient
        default:
            return .permanent(reason: "HTTP \(status)")
        }
    }

    /// Exponential backoff with deterministic jitter derived from the attempt
    /// number, so retry timing is reproducible in tests.
    public func delay(forAttempt attempt: Int) -> Duration {
        precondition(attempt >= 0, "attempt must be non-negative")
        let factor = Double(1 << Swift.min(attempt, 20))
        let jitter = 1.0 + Double((attempt &* 37) % 25) / 100.0
        let seconds = baseDelay.seconds * factor * jitter
        return Swift.min(.seconds(seconds), maxDelay)
    }
}

extension Duration {
    fileprivate var seconds: Double {
        Double(components.seconds) + Double(components.attoseconds) / 1e18
    }
}
