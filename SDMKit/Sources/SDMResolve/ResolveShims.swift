import Foundation

// UserDefaults stores these settings by their raw string; the generated enums
// carry no raw values.

extension CookieSource: CaseIterable, RawRepresentable {
    public static let allCases: [CookieSource] = (try? cookieSourceAll()) ?? []

    public init?(rawValue: String) {
        guard let match = try? cookieSourceFromRawValue(raw: rawValue) else { return nil }
        self = match
    }

    public var rawValue: String { (try? cookieSourceRawValue(source: self)) ?? "none" }
}

extension YtDlpChannel: CaseIterable, RawRepresentable {
    public static let allCases: [YtDlpChannel] = (try? ytDlpChannelAll()) ?? []

    public init?(rawValue: String) {
        guard let match = try? ytDlpChannelFromRawValue(raw: rawValue) else { return nil }
        self = match
    }

    public var rawValue: String { (try? ytDlpChannelRawValue(channel: self)) ?? "stable" }
}

extension VendorAsset {
    /// The old label for the compressed blob's location.
    public init(name: String, compressedURL: URL, version: String) {
        self.init(name: name, compressedPath: compressedURL, version: version)
    }
}
