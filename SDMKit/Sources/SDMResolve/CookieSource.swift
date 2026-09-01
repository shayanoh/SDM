import Foundation

/// Which browser's cookie jar yt-dlp should borrow for age-restricted,
/// private, or members-only videos. Parent spec §8.
public enum CookieSource: String, Sendable, Codable, Equatable, CaseIterable {
    case none, safari, chrome, firefox, edge, brave

    public var ytDlpArguments: [String] {
        switch self {
        case .none: return []
        default: return ["--cookies-from-browser", rawValue]
        }
    }
}
