import Foundation

/// Per-type toggles from spec §9.8. Mirrors `GrabberSettings`'s pattern.
@MainActor
enum NotificationSettings {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let downloadFinished = "sdm.notify.downloadFinished"
        static let packageFinished = "sdm.notify.packageFinished"
        static let downloadFailed = "sdm.notify.downloadFailed"
        static let linksGrabbed = "sdm.notify.linksGrabbed"
    }

    static var downloadFinishedEnabled: Bool {
        get { defaults.object(forKey: Key.downloadFinished) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.downloadFinished) }
    }

    static var packageFinishedEnabled: Bool {
        get { defaults.object(forKey: Key.packageFinished) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.packageFinished) }
    }

    static var downloadFailedEnabled: Bool {
        get { defaults.object(forKey: Key.downloadFailed) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.downloadFailed) }
    }

    static var linksGrabbedEnabled: Bool {
        get { defaults.object(forKey: Key.linksGrabbed) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.linksGrabbed) }
    }
}
