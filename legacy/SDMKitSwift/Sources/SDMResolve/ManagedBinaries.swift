import CryptoKit
import Foundation
import OSLog

private let log = Logger(subsystem: "com.sdm.SDMResolve", category: "managed-binaries")

/// Owns `~/Library/Application Support/SDM/bin/`: inflates the bundled
/// `ffmpeg` / `qjs` blobs, downloads and self-updates `yt-dlp`. Time is a
/// tick counter advanced ~1 Hz by the app — never a wall clock — so tests
/// drive cadence by calling `tick()` directly. See
/// `docs/superpowers/specs/2026-09-03-managed-binaries-design.md`.
public actor ManagedBinaries {
    static let presentInterval = 21_600  // 6 h at 1 Hz
    static let absentInterval = 900  // 15 min at 1 Hz

    private let binDirectory: URL
    private let fetcher: any BinaryFetching
    private let runner: any ProcessRunner
    private let vendorAssets: @Sendable () -> [VendorAsset]
    private let channel: @Sendable () -> YtDlpChannel
    private let onBinariesChanged: @Sendable () async -> Void
    private let notify: @Sendable (String) -> Void

    private var currentTick = 0
    private var lastCheckAtTick = 0
    private var latestKnown: String?
    private var inFlight: Task<CheckOutcome, Never>?

    private var ytDlpURL: URL { binDirectory.appendingPathComponent("yt-dlp") }
    private var manifestURL: URL { binDirectory.appendingPathComponent("manifest.json") }

    public init(
        binDirectory: URL,
        fetcher: any BinaryFetching,
        runner: any ProcessRunner,
        vendorAssets: @escaping @Sendable () -> [VendorAsset],
        channel: @escaping @Sendable () -> YtDlpChannel,
        onBinariesChanged: @escaping @Sendable () async -> Void = {},
        notify: @escaping @Sendable (String) -> Void = { _ in }
    ) {
        self.binDirectory = binDirectory
        self.fetcher = fetcher
        self.runner = runner
        self.vendorAssets = vendorAssets
        self.channel = channel
        self.onBinariesChanged = onBinariesChanged
        self.notify = notify
    }

    // MARK: - Bundled inflation

    /// Decode any bundled blob whose version differs from what the manifest
    /// records as installed. Cheap, safe to call on every launch.
    public func provisionBundledIfNeeded() async {
        var manifest = loadManifest()
        for asset in vendorAssets() {
            guard FileManager.default.fileExists(atPath: asset.compressedURL.path) else { continue }
            let target = binDirectory.appendingPathComponent(asset.name)
            let installed = manifest.version(for: asset.name)
            let present = FileManager.default.isExecutableFile(atPath: target.path)
            guard !present || installed != asset.version else { continue }
            do {
                let blob = try Data(contentsOf: asset.compressedURL)
                let binary = try LZFSE.decompress(blob)
                try writeExecutable(binary, to: target)
                switch asset.name {
                case "ffmpeg", "ffprobe": manifest.ffmpegVersion = asset.version
                case "qjs": manifest.qjsVersion = asset.version
                default: break
                }
                try saveManifest(manifest)
                log.info(
                    "inflated \(asset.name, privacy: .public) \(asset.version, privacy: .public)")
            } catch {
                manifest.lastError = "inflate \(asset.name): \(error)"
                try? saveManifest(manifest)
                log.error("inflate \(asset.name, privacy: .public) failed: \(error)")
            }
        }
    }

    // MARK: - Cadence

    /// Advance the clock one tick. Runs a check inline when the interval has
    /// elapsed and none is running. Called ~1 Hz by the app.
    public func tick() async {
        currentTick += 1
        let present = FileManager.default.isExecutableFile(atPath: ytDlpURL.path)
        let interval = present ? Self.presentInterval : Self.absentInterval
        guard inFlight == nil, currentTick - lastCheckAtTick >= interval else { return }
        _ = await checkNow(reason: .timer)
    }

    // MARK: - Update

    /// Check for a newer `yt-dlp` and download it if needed. Coalesces with
    /// any in-flight check — never runs two at once.
    @discardableResult
    public func checkNow(reason: CheckReason) async -> CheckOutcome {
        if let inFlight { return await inFlight.value }
        let task = Task { await self.performCheck(reason: reason) }
        inFlight = task
        let outcome = await task.value
        inFlight = nil
        return outcome
    }

    public func status() -> ManagedBinariesStatus {
        let manifest = loadManifest()
        return ManagedBinariesStatus(
            ytDlpVersion: manifest.ytDlpVersion,
            latestKnown: latestKnown ?? manifest.ytDlpVersion,
            channel: channel(),
            ffmpegVersion: manifest.ffmpegVersion,
            qjsVersion: manifest.qjsVersion,
            lastCheckTick: manifest.lastCheckTick == 0 ? nil : manifest.lastCheckTick,
            lastError: manifest.lastError)
    }

    var manifestForTesting: BinariesManifest { loadManifest() }

    private func performCheck(reason: CheckReason) async -> CheckOutcome {
        let ch = channel()
        var manifest = loadManifest()
        manifest.lastCheckTick = currentTick
        lastCheckAtTick = currentTick

        let tag: String
        do {
            let info = try await fetcher.data(from: ch.releasesAPI)
            tag = try JSONDecoder().decode(GitHubRelease.self, from: info).tagName
        } catch let error as BinaryFetchError {
            if case .transport = error {
                manifest.lastError = "offline: \(error)"
                try? saveManifest(manifest)
                return .noNetwork
            }
            manifest.lastError = "release info: \(error)"
            try? saveManifest(manifest)
            return .failed("\(error)")
        } catch {
            manifest.lastError = "release info: \(error)"
            try? saveManifest(manifest)
            return .failed("\(error)")
        }
        latestKnown = tag

        let haveBinary = FileManager.default.isExecutableFile(atPath: ytDlpURL.path)
        let needsDownload: Bool
        if !haveBinary {
            needsDownload = true
        } else if let installed = manifest.ytDlpVersion.flatMap(YtDlpVersion.init),
            let remote = YtDlpVersion(tag)
        {
            needsDownload = remote > installed
        } else {
            needsDownload = manifest.ytDlpVersion != tag
        }

        guard needsDownload else {
            manifest.lastError = nil
            try? saveManifest(manifest)
            return .upToDate(manifest.ytDlpVersion)
        }

        do {
            let bytes = try await fetcher.data(from: ch.assetURL(tag: tag))
            let sums = String(
                decoding: try await fetcher.data(from: ch.sumsURL(tag: tag)), as: UTF8.self)
            try verify(bytes, matches: sums, filename: "yt-dlp_macos")

            let staging = ytDlpURL.appendingPathExtension("new")
            try writeExecutable(bytes, to: staging)
            _ = try? await runner.run(
                executable: URL(fileURLWithPath: "/usr/bin/xattr"),
                arguments: ["-c", staging.path], timeout: .seconds(20))
            _ = try? await runner.run(
                executable: URL(fileURLWithPath: "/usr/bin/codesign"),
                arguments: ["-s", "-", "--force", staging.path], timeout: .seconds(30))
            try promote(staging, to: ytDlpURL)

            manifest.ytDlpVersion = tag
            manifest.ytDlpChannel = ch
            manifest.lastError = nil
            try saveManifest(manifest)
            await onBinariesChanged()
            notify("yt-dlp updated to \(tag)")
            log.info(
                "yt-dlp updated to \(tag, privacy: .public) (\(String(describing: reason), privacy: .public))"
            )
            return .updated(tag)
        } catch let error as BinaryFetchError {
            manifest.lastError = "download: \(error)"
            try? saveManifest(manifest)
            if case .transport = error { return .noNetwork }
            return .failed("\(error)")
        } catch {
            manifest.lastError = "\(error)"
            try? saveManifest(manifest)
            return .failed("\(error)")
        }
    }

    // MARK: - Helpers

    private func verify(_ bytes: Data, matches sums: String, filename: String) throws {
        let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        for line in sums.split(whereSeparator: \.isNewline) {
            let parts = line.split(separator: " ", omittingEmptySubsequences: true)
            guard parts.count >= 2 else { continue }
            let name = parts.last.map(String.init)?.trimmingCharacters(in: .whitespaces)
            if name == filename || name == "*\(filename)" {
                guard String(parts[0]).lowercased() == digest else {
                    throw ManagedBinariesError.checksumMismatch
                }
                return
            }
        }
        throw ManagedBinariesError.checksumLineMissing
    }

    private func writeExecutable(_ data: Data, to url: URL) throws {
        try FileManager.default.createDirectory(
            at: binDirectory, withIntermediateDirectories: true)
        if FileManager.default.fileExists(atPath: url.path) {
            try FileManager.default.removeItem(at: url)
        }
        try data.write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
    }

    private func promote(_ staging: URL, to destination: URL) throws {
        if FileManager.default.fileExists(atPath: destination.path) {
            _ = try FileManager.default.replaceItemAt(destination, withItemAt: staging)
        } else {
            try FileManager.default.moveItem(at: staging, to: destination)
        }
        try? FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: destination.path)
    }

    private func loadManifest() -> BinariesManifest {
        guard let data = try? Data(contentsOf: manifestURL),
            let manifest = try? JSONDecoder().decode(BinariesManifest.self, from: data)
        else { return .empty }
        return manifest
    }

    private func saveManifest(_ manifest: BinariesManifest) throws {
        try FileManager.default.createDirectory(
            at: binDirectory, withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(manifest).write(to: manifestURL, options: .atomic)
    }
}

enum ManagedBinariesError: Error, Equatable, Sendable {
    case checksumMismatch
    case checksumLineMissing
}

private struct GitHubRelease: Decodable {
    let tagName: String
    enum CodingKeys: String, CodingKey { case tagName = "tag_name" }
}
