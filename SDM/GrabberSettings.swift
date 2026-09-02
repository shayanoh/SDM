import Foundation

/// Backs the Phase 2 toggles from spec §12. There is no dedicated Settings
/// screen yet, so these read and write `UserDefaults` directly rather than
/// through a settings model that does not otherwise exist.
@MainActor
enum GrabberSettings {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let clipboardWatching = "sdm.clipboardWatchingEnabled"
        static let autoAddAndStart = "sdm.autoAddAndStartOnGrab"
        static let deepSniff = "sdm.deepSniffEnabled"
        static let autoClear = "sdm.autoClearGrabbedLinksAfter"
    }

    static var clipboardWatchingEnabled: Bool {
        get { defaults.object(forKey: Key.clipboardWatching) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.clipboardWatching) }
    }

    static var autoAddAndStartOnGrab: Bool {
        get { defaults.object(forKey: Key.autoAddAndStart) as? Bool ?? false }
        set { defaults.set(newValue, forKey: Key.autoAddAndStart) }
    }

    static var deepSniffEnabled: Bool {
        get { defaults.object(forKey: Key.deepSniff) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.deepSniff) }
    }

    /// How long an ungrabbed link/media row stays in the linkgrabber before
    /// it is cleared automatically. Default 30 minutes.
    enum AutoClearInterval: String, CaseIterable, Identifiable, Sendable {
        case fifteenMinutes, thirtyMinutes, oneHour, oneDay, never

        var id: String { rawValue }

        /// `nil` for `.never`.
        var seconds: TimeInterval? {
            switch self {
            case .fifteenMinutes: return 15 * 60
            case .thirtyMinutes: return 30 * 60
            case .oneHour: return 60 * 60
            case .oneDay: return 24 * 60 * 60
            case .never: return nil
            }
        }

        var label: String {
            switch self {
            case .fifteenMinutes: return "After 15 minutes"
            case .thirtyMinutes: return "After 30 minutes"
            case .oneHour: return "After 1 hour"
            case .oneDay: return "After 1 day"
            case .never: return "Never"
            }
        }
    }

    static var autoClearGrabbedLinksAfter: AutoClearInterval {
        get {
            AutoClearInterval(rawValue: defaults.string(forKey: Key.autoClear) ?? "")
                ?? .thirtyMinutes
        }
        set { defaults.set(newValue.rawValue, forKey: Key.autoClear) }
    }
}
