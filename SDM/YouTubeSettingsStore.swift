import Foundation
import SDMCore
import SDMResolve

/// Backs the YouTube quality/cookies/playlist settings from spec §12.
/// Deliberately a plain enum (not `@MainActor`) so the resolver's
/// `@Sendable` provider closures can read it directly — `UserDefaults` is
/// thread-safe. Mirrors `EngineSettingsStore`'s direct-`UserDefaults`
/// pattern.
enum YouTubeSettingsStore {
    private static let d = UserDefaults.standard

    private enum Key {
        static let maxHeight = "sdm.yt.maxHeight"
        static let allowAV1 = "sdm.yt.allowAV1"
        static let allowVP9 = "sdm.yt.allowVP9"
        static let allowH264 = "sdm.yt.allowH264"
        static let allowMP4 = "sdm.yt.allowMP4"
        static let allowWebM = "sdm.yt.allowWebM"
        static let allowOpus = "sdm.yt.allowOpus"
        static let allowAAC = "sdm.yt.allowAAC"
        static let maxPlaylistVideos = "sdm.yt.maxPlaylistVideos"
        static let cookieSourceRaw = "sdm.yt.cookieSource"
    }

    private static func bool(_ key: String) -> Bool { d.object(forKey: key) as? Bool ?? true }
    private static func setBool(_ value: Bool, _ key: String) { d.set(value, forKey: key) }

    /// The resolution ladder shown in Settings, highest first. `FormatSelector`
    /// treats the choice as a ceiling (`height <= maxHeight`).
    static let resolutionChoices = [1920, 1440, 1080, 720, 540, 480]

    static var maxHeight: Int {
        get {
            let raw = d.object(forKey: Key.maxHeight) as? Int ?? 1080
            // Snap a stored value onto the ladder so the picker always has a
            // selection (older builds stored arbitrary stepper values).
            return resolutionChoices.contains(raw)
                ? raw : (resolutionChoices.first { $0 <= raw } ?? 480)
        }
        set { d.set(newValue, forKey: Key.maxHeight) }
    }

    static var allowAV1: Bool {
        get { bool(Key.allowAV1) }
        set { setBool(newValue, Key.allowAV1) }
    }
    static var allowVP9: Bool {
        get { bool(Key.allowVP9) }
        set { setBool(newValue, Key.allowVP9) }
    }
    static var allowH264: Bool {
        get { bool(Key.allowH264) }
        set { setBool(newValue, Key.allowH264) }
    }
    static var allowMP4: Bool {
        get { bool(Key.allowMP4) }
        set { setBool(newValue, Key.allowMP4) }
    }
    static var allowWebM: Bool {
        get { bool(Key.allowWebM) }
        set { setBool(newValue, Key.allowWebM) }
    }
    static var allowOpus: Bool {
        get { bool(Key.allowOpus) }
        set { setBool(newValue, Key.allowOpus) }
    }
    static var allowAAC: Bool {
        get { bool(Key.allowAAC) }
        set { setBool(newValue, Key.allowAAC) }
    }

    static var maxPlaylistVideos: Int {
        get { min(200, max(10, d.object(forKey: Key.maxPlaylistVideos) as? Int ?? 50)) }
        set { d.set(min(200, max(10, newValue)), forKey: Key.maxPlaylistVideos) }
    }

    static var cookieSource: CookieSource {
        get { CookieSource(rawValue: d.string(forKey: Key.cookieSourceRaw) ?? "") ?? .none }
        set { d.set(newValue.rawValue, forKey: Key.cookieSourceRaw) }
    }

    /// Live-computed from the toggles. Fixed priority orders live in
    /// `FormatSelector`, not here. An all-empty set (a stale defaults file)
    /// falls back to "accept everything" so `FormatSelector` never sees an
    /// empty allowlist — the Settings UI also blocks unchecking the last.
    static var qualityPreferences: QualityPreferences {
        var video: Set<VideoCodec> = []
        if allowAV1 { video.insert(.av1) }
        if allowVP9 { video.insert(.vp9) }
        if allowH264 { video.insert(.h264) }
        var containers: Set<MediaContainer> = []
        if allowMP4 { containers.insert(.mp4) }
        if allowWebM { containers.insert(.webm) }
        var audio: Set<AudioCodec> = []
        if allowOpus { audio.insert(.opus) }
        if allowAAC { audio.insert(.aac) }
        return QualityPreferences(
            maxHeight: maxHeight,
            videoCodecs: video.isEmpty ? [.av1, .vp9, .h264] : video,
            containers: containers.isEmpty ? [.mp4, .webm] : containers,
            audioCodecs: audio.isEmpty ? [.opus, .aac] : audio)
    }
}
