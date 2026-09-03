import Foundation

/// yt-dlp release channel. Stable ships ~monthly; nightly publishes on any
/// day the codebase changes and is the safer choice against YouTube breakage.
public enum YtDlpChannel: String, Codable, Sendable, CaseIterable {
    case stable
    case nightly

    var repo: String {
        switch self {
        case .stable: "yt-dlp/yt-dlp"
        case .nightly: "yt-dlp/yt-dlp-nightly-builds"
        }
    }

    /// GitHub "latest release" API for this channel.
    public var releasesAPI: URL {
        URL(string: "https://api.github.com/repos/\(repo)/releases/latest")!
    }

    /// Direct download URL for the universal2 macOS binary of a given tag.
    func assetURL(tag: String) -> URL {
        URL(string: "https://github.com/\(repo)/releases/download/\(tag)/yt-dlp_macos")!
    }

    /// The `SHA2-256SUMS` manifest for a given tag.
    func sumsURL(tag: String) -> URL {
        URL(string: "https://github.com/\(repo)/releases/download/\(tag)/SHA2-256SUMS")!
    }
}

/// A yt-dlp version tag: `YYYY.MM.DD` (stable) or `YYYY.MM.DD.HHMMSS`
/// (nightly). Compared component-wise, shorter padded with zeros.
public struct YtDlpVersion: Comparable, Sendable, CustomStringConvertible {
    public let components: [Int]

    public init?(_ tag: String) {
        let trimmed = tag.trimmingCharacters(in: .whitespacesAndNewlines)
        let parts = trimmed.split(separator: ".", omittingEmptySubsequences: false)
        guard !parts.isEmpty else { return nil }
        var parsed: [Int] = []
        for part in parts {
            guard let value = Int(part) else { return nil }
            parsed.append(value)
        }
        components = parsed
    }

    public static func < (lhs: YtDlpVersion, rhs: YtDlpVersion) -> Bool {
        let count = max(lhs.components.count, rhs.components.count)
        for index in 0..<count {
            let l = index < lhs.components.count ? lhs.components[index] : 0
            let r = index < rhs.components.count ? rhs.components[index] : 0
            if l != r { return l < r }
        }
        return false
    }

    public var description: String { components.map(String.init).joined(separator: ".") }
}

/// Persisted at `<binDir>/manifest.json`. Tracks what is installed so a
/// launch check knows whether to download.
public struct BinariesManifest: Codable, Sendable, Equatable {
    public var ytDlpVersion: String?
    public var ytDlpChannel: YtDlpChannel
    public var lastCheckTick: Int
    public var ffmpegVersion: String?
    public var qjsVersion: String?
    public var lastError: String?

    public static var empty: BinariesManifest {
        BinariesManifest(
            ytDlpVersion: nil, ytDlpChannel: .stable, lastCheckTick: 0,
            ffmpegVersion: nil, qjsVersion: nil, lastError: nil)
    }

    func version(for assetName: String) -> String? {
        switch assetName {
        case "ffmpeg", "ffprobe": ffmpegVersion
        case "qjs": qjsVersion
        default: nil
        }
    }
}

/// A binary shipped compressed inside the `.app` bundle, to be inflated into
/// the managed `bin/` directory. `name` is `"ffmpeg"` or `"qjs"`.
public struct VendorAsset: Sendable {
    public var name: String
    public var compressedURL: URL
    public var version: String

    public init(name: String, compressedURL: URL, version: String) {
        self.name = name
        self.compressedURL = compressedURL
        self.version = version
    }
}

public enum CheckReason: Sendable {
    case launch
    case timer
    case resolveNeeded
    case channelChanged
    case manual
}

public enum CheckOutcome: Sendable, Equatable {
    case upToDate(String?)
    case updated(String)
    case noNetwork
    case failed(String)
    case skipped
}

/// Read by the Settings "Components" section.
public struct ManagedBinariesStatus: Sendable, Equatable {
    public var ytDlpVersion: String?
    public var latestKnown: String?
    public var channel: YtDlpChannel
    public var ffmpegVersion: String?
    public var qjsVersion: String?
    public var lastCheckTick: Int?
    public var lastError: String?

    public init(
        ytDlpVersion: String?, latestKnown: String?, channel: YtDlpChannel,
        ffmpegVersion: String?, qjsVersion: String?, lastCheckTick: Int?, lastError: String?
    ) {
        self.ytDlpVersion = ytDlpVersion
        self.latestKnown = latestKnown
        self.channel = channel
        self.ffmpegVersion = ffmpegVersion
        self.qjsVersion = qjsVersion
        self.lastCheckTick = lastCheckTick
        self.lastError = lastError
    }
}
