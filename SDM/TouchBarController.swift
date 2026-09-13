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

        let bar: NSTouchBar?
        if isAnyRunning || hasWaitingWork {
            let installed = touchBar ?? makeTouchBar()
            touchBar = installed
            bar = installed
        } else {
            bar = nil
        }

        NSApp.touchBar = bar
        for window in NSApp.windows {
            window.touchBar = bar
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
