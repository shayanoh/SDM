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
        static let autoStartDownloadsOnLaunch = "sdm.autoStartDownloadsOnLaunch"
        static let minSegmentSizeMB = "sdm.minSegmentSizeMB"
        static let chromeSetupDialogShowCount = "sdm.chromeSetupDialogShowCount"
        static let chromeSetupDialogLastVersion = "sdm.chromeSetupDialogShowLastVersion"
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

    /// Stored in MB (the Settings UI's unit); converted to bytes for
    /// `EngineSettings.minSegmentSizeBytes`. Range enforced by the Settings
    /// field itself (1...100).
    static var minSegmentSizeMB: Int {
        get { defaults.object(forKey: Key.minSegmentSizeMB) as? Int ?? 10 }
        set { defaults.set(newValue, forKey: Key.minSegmentSizeMB) }
    }

    /// Off by default: a freshly launched app should not start pulling bytes
    /// until the operator says so. When `true`, `EngineController` calls
    /// `DownloadEngine.resumeAll()` once right after `restore()` — the same
    /// call Resume All uses, requeuing every enabled, `.stopped` item and
    /// leaving disabled ones alone. When `false`, nothing is called and
    /// everything simply stays `.stopped`, exactly as `restore()` left it.
    static var autoStartDownloadsOnLaunch: Bool {
        get { defaults.object(forKey: Key.autoStartDownloadsOnLaunch) as? Bool ?? false }
        set { defaults.set(newValue, forKey: Key.autoStartDownloadsOnLaunch) }
    }

    static var chromeSetupDialogShowCount: Int {
        get { defaults.object(forKey: Key.chromeSetupDialogShowCount) as? Int ?? 0 }
        set { defaults.set(newValue, forKey: Key.chromeSetupDialogShowCount) }
    }

    static var chromeSetupDialogLastVersion: String {
        get { defaults.object(forKey: Key.chromeSetupDialogLastVersion) as? String ?? "" }
        set { defaults.set(newValue, forKey: Key.chromeSetupDialogLastVersion) }
    }
}
