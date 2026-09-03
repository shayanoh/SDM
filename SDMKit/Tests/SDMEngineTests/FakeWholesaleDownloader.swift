import Foundation
import SDMCore

@testable import SDMEngine

/// In-process `WholesaleDownloader` a test drives step by step — mirrors
/// `FakeOrigin`. `download` blocks until `finishSuccess()` / `finishFailure()`;
/// `emit` pushes a progress report; cancellation resolves cleanly.
final class FakeWholesaleDownloader: WholesaleDownloader, @unchecked Sendable {
    private let lock = NSLock()
    private var onProgress: (@Sendable (WholesaleProgress) -> Void)?
    private var continuation: CheckedContinuation<Void, any Error>?
    private var startedWaiter: CheckedContinuation<Void, Never>?
    private var destination: URL?
    private var cancelled = false

    private(set) var callCount = 0

    func download(
        pageURL: URL, formatSelector: String, destination: URL,
        onProgress: @Sendable @escaping (WholesaleProgress) -> Void
    ) async throws {
        lock.withLock {
            self.onProgress = onProgress
            self.destination = destination
            self.cancelled = false
            callCount += 1
            startedWaiter?.resume()
            startedWaiter = nil
        }
        defer { lock.withLock { self.onProgress = nil } }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation {
                (c: CheckedContinuation<Void, any Error>) in
                let resumeNow: Bool = lock.withLock {
                    if cancelled { return true }
                    self.continuation = c
                    return false
                }
                if resumeNow { c.resume(throwing: CancellationError()) }
            }
        } onCancel: {
            let c: CheckedContinuation<Void, any Error>? = lock.withLock {
                cancelled = true
                let c = continuation
                continuation = nil
                return c
            }
            c?.resume(throwing: CancellationError())
        }
    }

    /// Suspends until `download` has been called and registered its callback.
    func waitUntilStarted() async {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            let ready: Bool = lock.withLock {
                if onProgress != nil { return true }
                startedWaiter = c
                return false
            }
            if ready { c.resume() }
        }
    }

    func emit(_ progress: WholesaleProgress) {
        let cb = lock.withLock { onProgress }
        cb?(progress)
    }

    func finishSuccess() {
        if let dest = lock.withLock({ destination }) {
            FileManager.default.createFile(atPath: dest.path, contents: Data("wholesale".utf8))
        }
        resumeContinuation(with: nil)
    }

    func finishFailure(_ error: WholesaleError) {
        resumeContinuation(with: error)
    }

    private func resumeContinuation(with error: (any Error)?) {
        let c: CheckedContinuation<Void, any Error>? = lock.withLock {
            let c = continuation
            continuation = nil
            return c
        }
        if let error { c?.resume(throwing: error) } else { c?.resume() }
    }
}
