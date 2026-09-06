import Observation
import ServiceManagement

/// Spec-analogous to `ActivationPolicyController`: an `@Observable` wrapper
/// so a `Toggle` bound to `isEnabled` applies immediately, with no separate
/// "Apply" step. Unlike activation policy there is no `UserDefaults`-backed
/// store of our own — `SMAppService.mainApp.status` is already durable and
/// is the single source of truth, so reading it back on `init()` is enough
/// to reflect whatever the user (or a previous install) last set. Disabled
/// by default: a freshly installed app is `.notRegistered` until the user
/// opts in.
@MainActor
@Observable
final class LaunchAtLoginController {
    var isEnabled: Bool {
        didSet {
            guard isEnabled != oldValue else { return }
            apply(isEnabled)
        }
    }

    init() {
        isEnabled = SMAppService.mainApp.status == .enabled
    }

    private func apply(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            print("Failed to \(enabled ? "register" : "unregister") launch-at-login item: \(error)")
            // Roll the toggle back to whatever actually took effect, without
            // re-entering `apply` (the guard in `didSet` only fires on a
            // genuine change).
            isEnabled = SMAppService.mainApp.status == .enabled
        }
    }
}
