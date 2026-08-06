import Foundation

enum ActivationPolicyMode: String, CaseIterable, Identifiable {
    case menuBarOnly, dockOnly, both

    var id: String { rawValue }

    var label: String {
        switch self {
        case .menuBarOnly: return "Menu Bar Only"
        case .dockOnly: return "Dock Only"
        case .both: return "Both"
        }
    }
}

/// Spec §12: "Dock / menu bar mode", default `.both`. Mirrors
/// `GrabberSettings`'s direct-`UserDefaults` pattern.
@MainActor
enum ActivationPolicyStore {
    private static let defaults = UserDefaults.standard
    private static let key = "sdm.activationPolicyMode"

    static var mode: ActivationPolicyMode {
        get { ActivationPolicyMode(rawValue: defaults.string(forKey: key) ?? "") ?? .both }
        set { defaults.set(newValue.rawValue, forKey: key) }
    }
}
