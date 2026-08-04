import AppKit
import Foundation
import SDMGrabber

/// Polls `NSPasteboard.changeCount`, since `NSPasteboard` has no change
/// notification. Spec §7.1.
///
/// `NSPasteboard.detectedValues(for:)` — the privacy-preserving read that
/// returns a probable URL without exposing full pasteboard content — was
/// confirmed against the macOS 26 SDK's `AppKit.swiftinterface`:
/// `@available(macOS 15.4, *) func detectedValues(for:) async throws ->
/// NSPasteboard.DetectedValues`, with a non-optional `probableWebURL:
/// String`. That is above this project's macOS 15.0 baseline, so it is used
/// behind an availability check; below 15.4 this falls back to a full
/// `.string(forType: .string)` read passed through `URLExtractor`. Re-check
/// this gap against whatever SDK is installed at implementation time —
/// Apple could backport the API to an earlier 15.x point release.
@MainActor
final class ClipboardWatcher {
    private let pasteboard = NSPasteboard.general
    private var lastChangeCount: Int
    private var lastIgnoredURLs: Set<URL> = []
    private var timer: Timer?

    /// Called with newly detected URLs from content SDM did not itself just
    /// place on the pasteboard.
    var onLinksDetected: (([URL]) -> Void)?

    init() {
        lastChangeCount = pasteboard.changeCount
    }

    func start() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            Task { await self.poll() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// Marks URLs SDM itself just placed on the pasteboard, so copying a
    /// link back out of the app does not re-grab it. Spec §7.1.
    func ignoreOwnWrite(_ urls: [URL]) {
        lastIgnoredURLs = Set(urls)
    }

    private func poll() async {
        guard pasteboard.changeCount != lastChangeCount else { return }
        lastChangeCount = pasteboard.changeCount

        let urls = await detectURLs()
        guard !urls.isEmpty, Set(urls) != lastIgnoredURLs else { return }
        onLinksDetected?(urls)
    }

    private func detectURLs() async -> [URL] {
        if #available(macOS 15.4, *) {
            guard
                let detected = try? await pasteboard.detectedValues(for: [\.probableWebURL]),
                !detected.probableWebURL.isEmpty
            else { return [] }
            return URLExtractor.extractLinks(from: detected.probableWebURL)
        }
        guard let text = pasteboard.string(forType: .string) else { return [] }
        return URLExtractor.extractLinks(from: text)
    }
}
