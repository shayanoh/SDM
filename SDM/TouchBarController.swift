import AppKit
import SDMCore
import SDMEngine
import SwiftUI

/// Installs SDM's Touch Bar item directly on `NSApp` — a `var` inherited
/// from `NSResponder` — rather than overriding `touchBar` on any window or
/// view controller. None of SDM's windows provide a custom `touchBar`, so
/// `NSApp`'s is the one AppKit falls back to at the tail of the responder
/// chain: it becomes the effective bar whenever SDM is the active app,
/// regardless of which window (if any) is key.
///
/// Setting `NSApp.touchBar` is an inert no-op on the overwhelming majority
/// of Macs, which have no Touch Bar hardware, and behaves identically in
/// the Touch Bar Simulator — no availability guard is needed.
///
/// `update()` is called from the same `.onChange(of: controller.snapshot)`
/// hook `SDMApp` already uses to refresh the menu bar icon, so this needs no
/// timer of its own. It only ever decides *whether* the bar is installed;
/// once installed, `TouchBarContentView`'s own `@Observable` reads keep its
/// content live.
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
    private static let mainItemIdentifier = NSTouchBarItem.Identifier(
        "com.shayanoh.SDM.touchbar.main")

    private weak var controller: EngineController?
    private weak var themeStore: ThemeStore?
    private var touchBar: NSTouchBar?

    func configure(controller: EngineController, themeStore: ThemeStore) {
        self.controller = controller
        self.themeStore = themeStore
    }

    func update() {
        guard let controller else { return }
        let items = controller.snapshot.packages.flatMap(\.items)
        let isAnyRunning = items.contains { $0.state == .running }
        let hasWaitingWork = items.contains { $0.isEnabled && $0.state != .completed }

        guard isAnyRunning || hasWaitingWork else {
            NSApp.touchBar = nil
            return
        }

        let bar = touchBar ?? makeTouchBar()
        touchBar = bar
        if NSApp.touchBar !== bar {
            NSApp.touchBar = bar
        }
    }

    private func makeTouchBar() -> NSTouchBar {
        let bar = NSTouchBar()
        bar.delegate = self
        bar.defaultItemIdentifiers = [Self.mainItemIdentifier]
        return bar
    }

    func touchBar(
        _ touchBar: NSTouchBar, makeItemForIdentifier identifier: NSTouchBarItem.Identifier
    ) -> NSTouchBarItem? {
        guard identifier == Self.mainItemIdentifier, let controller, let themeStore else {
            return nil
        }
        let item = NSCustomTouchBarItem(identifier: identifier)
        let hosting = NSHostingView(
            rootView: TouchBarContentView()
                .environment(controller)
                .environment(themeStore)
        )
        hosting.frame = CGRect(x: 0, y: 0, width: 480, height: 30)
        item.view = hosting
        return item
    }
}
