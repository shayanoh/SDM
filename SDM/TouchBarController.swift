import AppKit
import SDMCore
import SDMEngine
import SwiftUI

/// Installs SDM's Touch Bar item on `NSApp` **and** on every open `NSWindow`.
/// Both are necessary: AppKit resolves the Touch Bar by walking the *key
/// window's* responder chain (first responder → … → the window itself) and
/// only falls back to `NSApp` when there is **no key window at all** (e.g.
/// accessory/menu-bar-only mode). Setting `NSApp.touchBar` alone is
/// therefore invisible the moment any SDM window has focus — which is the
/// common case — since that window's own chain resolves first and never
/// reaches `NSApp`. Setting `.touchBar` directly on each window (the same
/// shared `NSTouchBar` instance — harmless, since only the one actually-key
/// window's copy is ever realized on hardware) covers that case; `NSApp`'s
/// copy covers the no-window-key case.
///
/// Setting these is an inert no-op on the overwhelming majority of Macs,
/// which have no Touch Bar hardware, and behaves identically in the Touch
/// Bar Simulator — no availability guard is needed.
///
/// `update()` is called from the same `.onChange(of: controller.snapshot)`
/// hook `SDMApp` already uses to refresh the menu bar icon, so this needs no
/// timer of its own. It only ever decides *whether* the bar is installed;
/// once installed, `TouchBarContentView`'s own `@Observable` reads keep its
/// display content live on their own — the pause/resume button below is the
/// one thing this class must refresh by hand, since it's a plain AppKit
/// control, not SwiftUI.
///
/// Three states, re-evaluated on every call:
/// 1. **Downloading** (any item `.running`) — the bar is installed with full
///    content.
/// 2. **Idle, work pending** (nothing running, but some item is
///    `isEnabled && state != .completed`) — the bar stays installed;
///    `TouchBarContentView` itself switches to the resume-icon-and-count
///    display.
/// 3. **Fully idle** — `NSApp.touchBar = nil`, releasing the strip back to
///    the system Control Strip default rather than showing an empty bar.
@MainActor
final class TouchBarController: NSObject, NSTouchBarDelegate {
    private static let pauseResumeItemIdentifier = NSTouchBarItem.Identifier(
        "com.shayanoh.SDM.touchbar.pauseResume")
    private static let displayItemIdentifier = NSTouchBarItem.Identifier(
        "com.shayanoh.SDM.touchbar.display")

    private weak var controller: EngineController?
    private weak var themeStore: ThemeStore?
    private var touchBar: NSTouchBar?
    /// Weak: the item (and its button) is owned by `touchBar`, not this
    /// property — this is only a shortcut to refresh its image/enabled
    /// state each `update()` without re-walking `touchBar.item(forIdentifier:)`.
    private weak var pauseResumeItem: NSButtonTouchBarItem?

    func configure(controller: EngineController, themeStore: ThemeStore) {
        self.controller = controller
        self.themeStore = themeStore
    }

    func update() {
        guard let controller else { return }
        let items = controller.snapshot.packages.flatMap(\.items)
        let isAnyRunning = items.contains { $0.state == .running }
        let hasWaitingWork = items.contains { $0.isEnabled && $0.state != .completed }

        let bar: NSTouchBar?
        if isAnyRunning || hasWaitingWork {
            let installed = touchBar ?? makeTouchBar()
            touchBar = installed
            bar = installed
        } else {
            bar = nil
        }

        // Reassigning `.touchBar` to the *same* instance still costs AppKit a
        // teardown/rebuild of the realized item view — at `update()`'s tick
        // rate that reads as constant flicker. Guard by identity so this
        // only touches AppKit when the bar has actually changed.
        if NSApp.touchBar !== bar {
            NSApp.touchBar = bar
        }
        for window in NSApp.windows where window.touchBar !== bar {
            window.touchBar = bar
        }

        updatePauseResumeItem(items: items)
    }

    /// Mirrors `PackagesBottomBar.downloadableItems`/`allDownloadableStopped`
    /// (`PackagesListView.swift:1180-1193`) exactly — this button must behave
    /// identically to the one on the downloads tab.
    private func downloadableItems(_ items: [ItemSnapshot]) -> [ItemSnapshot] {
        items.filter {
            switch $0.state {
            case .queued, .running, .stopped: return true
            case .completed, .failed: return false
            }
        }
    }

    private func updatePauseResumeItem(items: [ItemSnapshot]) {
        guard let pauseResumeItem else { return }
        let downloadable = downloadableItems(items)
        let allStopped = downloadable.allSatisfy { $0.state == .stopped }
        pauseResumeItem.image =
            NSImage(
                systemSymbolName: allStopped ? "play.fill" : "pause.fill",
                accessibilityDescription: allStopped ? "Resume All" : "Pause All")
            ?? NSImage()
        pauseResumeItem.isEnabled = !downloadable.isEmpty
    }

    private func makeTouchBar() -> NSTouchBar {
        let bar = NSTouchBar()
        bar.delegate = self
        bar.defaultItemIdentifiers = [Self.pauseResumeItemIdentifier, Self.displayItemIdentifier]
        return bar
    }

    func touchBar(
        _ touchBar: NSTouchBar, makeItemForIdentifier identifier: NSTouchBarItem.Identifier
    ) -> NSTouchBarItem? {
        switch identifier {
        case Self.pauseResumeItemIdentifier:
            return makePauseResumeItem(identifier: identifier)
        case Self.displayItemIdentifier:
            return makeDisplayItem(identifier: identifier)
        default:
            return nil
        }
    }

    /// A native `NSButtonTouchBarItem`, not a SwiftUI `Button` hosted the way
    /// `TouchBarContentView` is — a hosted SwiftUI button highlights on
    /// touch-down but its `action` never fires in a Touch Bar item context.
    private func makePauseResumeItem(identifier: NSTouchBarItem.Identifier) -> NSTouchBarItem? {
        guard let controller else { return nil }
        let image =
            NSImage(systemSymbolName: "pause.fill", accessibilityDescription: "Pause All")
            ?? NSImage()
        let item = NSButtonTouchBarItem(
            identifier: identifier, image: image, target: self,
            action: #selector(pauseResumeTapped))
        pauseResumeItem = item
        updatePauseResumeItem(items: controller.snapshot.packages.flatMap(\.items))
        return item
    }

    private func makeDisplayItem(identifier: NSTouchBarItem.Identifier) -> NSTouchBarItem? {
        guard let controller, let themeStore else { return nil }
        let item = NSCustomTouchBarItem(identifier: identifier)
        // `TouchBarContentView` fixes its own `.frame(width:height:)` at its
        // root, so `NSHostingView`'s intrinsic content size already comes
        // out right — no need to also set `.frame` here.
        item.view = NSHostingView(
            rootView: TouchBarContentView()
                .environment(controller)
                .environment(themeStore)
        )
        return item
    }

    @objc private func pauseResumeTapped(_ sender: Any) {
        guard let controller else { return }
        let downloadable = downloadableItems(controller.snapshot.packages.flatMap(\.items))
        let allStopped = downloadable.allSatisfy { $0.state == .stopped }
        Task {
            if allStopped {
                await controller.resumeAll()
            } else {
                await controller.pauseAll()
            }
        }
    }
}
