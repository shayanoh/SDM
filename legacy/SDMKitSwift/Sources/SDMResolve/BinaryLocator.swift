import Foundation

/// Finds `yt-dlp` / `ffmpeg`: a Settings override first, then a fixed list
/// of common install locations. Parent spec §4.5. A `.app` launched from
/// Finder has no shell `PATH`, so callers always pass the absolute path
/// this returns.
public actor BinaryLocator {
    public static var defaultSearchPaths: [URL] {
        [
            "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin",
            NSString(string: "~/.local/bin").expandingTildeInPath,
        ].map { URL(fileURLWithPath: $0) }
    }

    private let searchPaths: [URL]
    private let isExecutable: @Sendable (URL) -> Bool
    private var overrides: [String: URL] = [:]
    private var memo: [String: URL?] = [:]

    public init(
        searchPaths: [URL] = BinaryLocator.defaultSearchPaths,
        isExecutable: @escaping @Sendable (URL) -> Bool = {
            FileManager.default.isExecutableFile(atPath: $0.path)
        }
    ) {
        self.searchPaths = searchPaths
        self.isExecutable = isExecutable
    }

    public func setOverride(_ url: URL?, for name: String) {
        overrides[name] = url
        memo.removeValue(forKey: name)
    }

    public func invalidate() {
        memo.removeAll()
    }

    public func locate(_ name: String) -> URL? {
        if let cached = memo[name] { return cached }
        let resolved = resolve(name)
        memo[name] = .some(resolved)
        return resolved
    }

    private func resolve(_ name: String) -> URL? {
        if let override = overrides[name], isExecutable(override) { return override }
        for directory in searchPaths {
            let candidate = directory.appendingPathComponent(name)
            if isExecutable(candidate) { return candidate }
        }
        return nil
    }
}
