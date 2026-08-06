import AppKit
import Observation

/// Applies spec §10.2's dock/menu-bar mode to `NSApp`'s activation policy.
/// Mirrors `EngineController`'s shape: an `@Observable` wrapper around a
/// `UserDefaults`-backed store, so a `Picker` bound to `mode` applies
/// immediately with no separate "Apply" step — spec §10.2: "Changing the
/// mode applies immediately, including when no window is open."
@MainActor
@Observable
final class ActivationPolicyController {
    var mode: ActivationPolicyMode {
        didSet {
            guard mode != oldValue else { return }
            ActivationPolicyStore.mode = mode
            apply()
        }
    }

    init() {
        mode = ActivationPolicyStore.mode
    }

    /// Applies the current mode's dock-icon policy. Safe to call any time,
    /// including from `AppDelegate`'s window-open/close observers.
    func apply() {
        switch mode {
        case .dockOnly, .both:
            NSApp.setActivationPolicy(.regular)
        case .menuBarOnly:
            // Only `.accessory` while genuinely windowless — a mode switch
            // made while a window is open should not yank the dock icon out
            // from under it.
            let hasVisibleWindow = NSApp.windows.contains { $0.isVisible }
            NSApp.setActivationPolicy(hasVisibleWindow ? .regular : .accessory)
        }
    }

    /// Spec §10.2: "Dock only... hidden" menu bar icon. Every other mode
    /// shows it. Bound to `MenuBarExtra(isInserted:)` in `SDMApp`.
    var showsMenuBarIcon: Bool { mode != .dockOnly }
}
