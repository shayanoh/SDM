import Foundation
import Observation
import SDMResolve

/// Bridges `SDMResolve.ManagedBinaries` to the app: owns the one shared
/// `BinaryLocator`, runs the launch check + 1 Hz tick loop, and republishes
/// a `ManagedBinariesStatus` for the Settings "Components" section.
@MainActor
@Observable
final class ManagedBinariesController {
    let binaryLocator: BinaryLocator
    private let managed: ManagedBinaries
    private var tickTask: Task<Void, Never>?

    private(set) var status = ManagedBinariesStatus(
        ytDlpVersion: nil, latestKnown: nil, channel: .stable,
        ffmpegVersion: nil, qjsVersion: nil, lastCheckTick: nil, lastError: nil)

    /// `~/Library/Application Support/SDM/bin`.
    nonisolated static var binDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM/bin", isDirectory: true)
    }

    /// The tokens every yt-dlp invocation carries: QuickJS as the JS runtime
    /// (off by default in yt-dlp) and the managed `bin/` as the ffmpeg
    /// location for any future yt-dlp-as-downloader path.
    nonisolated static var ytDlpExtraArguments: [String] {
        [
            "--extractor-args", "youtube:jsruntime=quickjs",
            "--ffmpeg-location", binDirectory.path,
        ]
    }

    init(notify: @escaping @Sendable (String) -> Void = { _ in }) {
        let bin = Self.binDirectory
        let locator = BinaryLocator(searchPaths: [bin])
        binaryLocator = locator
        managed = ManagedBinaries(
            binDirectory: bin,
            fetcher: URLSessionBinaryFetcher(),
            runner: SystemProcessRunner(),
            vendorAssets: { Self.bundledVendorAssets() },
            channel: { YouTubeSettingsStore.ytDlpChannel },
            onBinariesChanged: { await locator.invalidate() },
            notify: notify)
    }

    func start() {
        Task { [managed] in
            await managed.provisionBundledIfNeeded()
            _ = await managed.checkNow(reason: .launch)
            self.status = await managed.status()
        }
        tickTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                await self.managed.tick()
                self.status = await self.managed.status()
            }
        }
    }

    /// Settings "Check now" / channel change.
    func checkNow() async {
        _ = await managed.checkNow(reason: .manual)
        status = await managed.status()
    }

    /// Fire-and-forget: the grabber is about to resolve links and may need
    /// yt-dlp present. Coalesces with any in-flight check.
    func kick() {
        Task {
            _ = await managed.checkNow(reason: .resolveNeeded)
            status = await managed.status()
        }
    }

    nonisolated private static func bundledVendorAssets() -> [VendorAsset] {
        guard let dir = Bundle.main.url(forResource: "vendor", withExtension: nil) else {
            return []
        }
        let manifestURL = dir.appendingPathComponent("vendor-manifest.json")
        guard let data = try? Data(contentsOf: manifestURL),
            let versions = try? JSONDecoder().decode([String: String].self, from: data)
        else { return [] }

        #if arch(arm64)
            let ffmpegBlob = "ffmpeg-arm64.lzfse"
        #else
            let ffmpegBlob = "ffmpeg-x86_64.lzfse"
        #endif

        var assets: [VendorAsset] = []
        if let v = versions["ffmpegVersion"], v != "0" {
            assets.append(
                VendorAsset(
                    name: "ffmpeg", compressedURL: dir.appendingPathComponent(ffmpegBlob),
                    version: v))
        }
        if let v = versions["qjsVersion"], v != "0" {
            assets.append(
                VendorAsset(
                    name: "qjs", compressedURL: dir.appendingPathComponent("qjs.lzfse"),
                    version: v))
        }
        return assets
    }
}
