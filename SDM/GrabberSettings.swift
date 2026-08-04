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
}
