import Foundation

// UserDefaults stores these settings by their raw string; the generated enums
// carry no raw values.

extension CookieSource: CaseIterable, RawRepresentable {
    public static let allCases: [CookieSource] = [.none, .safari, .chrome, .firefox, .edge, .brave]

    public init?(rawValue: String) {
        guard let match = Self.allCases.first(where: { $0.rawValue == rawValue }) else {
            return nil
        }
        self = match
    }

    public var rawValue: String {
        switch self {
        case .none: "none"
        case .safari: "safari"
        case .chrome: "chrome"
        case .firefox: "firefox"
        case .edge: "edge"
        case .brave: "brave"
        }
    }
}

extension YtDlpChannel: RawRepresentable {
    public init?(rawValue: String) {
        switch rawValue {
        case "stable": self = .stable
        case "nightly": self = .nightly
        default: return nil
        }
    }

    public var rawValue: String {
        switch self {
        case .stable: "stable"
        case .nightly: "nightly"
        }
    }
}

extension VendorAsset {
    /// The old label for the compressed blob's location.
    public init(name: String, compressedURL: URL, version: String) {
        self.init(name: name, compressedPath: compressedURL, version: version)
    }
}
