import Foundation

/// Backs the engine-side settings from spec §12: max concurrent downloads,
/// segments per file, and the two connection ceilings. Mirrors
/// `GrabberSettings`'s direct-`UserDefaults` pattern — there is still no
/// single unified Settings model, just per-area stores.
@MainActor
enum EngineSettingsStore {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let maxConcurrent = "sdm.maxConcurrentDownloads"
        static let segmentsPerItem = "sdm.segmentsPerItem"
        static let globalMaxConnections = "sdm.globalMaxConnections"
        static let maxConnectionsPerHost = "sdm.maxConnectionsPerHost"
    }

    static var maxConcurrent: Int {
        get { defaults.object(forKey: Key.maxConcurrent) as? Int ?? 3 }
        set { defaults.set(newValue, forKey: Key.maxConcurrent) }
    }

    static var segmentsPerItem: Int {
        get { defaults.object(forKey: Key.segmentsPerItem) as? Int ?? 8 }
        set { defaults.set(newValue, forKey: Key.segmentsPerItem) }
    }

    static var globalMaxConnections: Int {
        get { defaults.object(forKey: Key.globalMaxConnections) as? Int ?? 32 }
        set { defaults.set(newValue, forKey: Key.globalMaxConnections) }
    }

    static var maxConnectionsPerHost: Int {
        get { defaults.object(forKey: Key.maxConnectionsPerHost) as? Int ?? 8 }
        set { defaults.set(newValue, forKey: Key.maxConnectionsPerHost) }
    }
}
