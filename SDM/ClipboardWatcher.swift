import AppKit
import Foundation
import SDMGrabber

/// Polls `NSPasteboard.changeCount`, since `NSPasteboard` has no change
/// notification. Spec §7.1.
///
/// Reads the pasteboard as a full string and runs it through `URLExtractor`
/// rather than `NSPasteboard.detectedValues(for:)`. That API was tried
/// first for its privacy properties — it avoids a full content read — but
/// `DetectedValues.probableWebURL` is `Swift.String`, singular by design
/// (the same "paste and go" property Safari uses for one suggested URL), not
/// a collection. Confirmed by hand: pasting several links at once and
/// grabbing only the first is exactly what that type signature predicts,
/// and it makes the API structurally unusable here — spec §7.5's batch-paste
/// flow requires catching every link on the pasteboard, not just one.
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
            self?.poll()
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

    private func poll() {
        guard pasteboard.changeCount != lastChangeCount else { return }
        lastChangeCount = pasteboard.changeCount

        let urls = detectURLs()
        guard !urls.isEmpty, Set(urls) != lastIgnoredURLs else { return }
        onLinksDetected?(urls)
    }

    private func detectURLs() -> [URL] {
        guard let text = pasteboard.string(forType: .string) else { return [] }
        return URLExtractor.extractLinks(from: text)
    }
}
