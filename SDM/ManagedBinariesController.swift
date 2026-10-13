import Foundation
import Observation
import SDMResolve
import os.log

/// Bridges the Rust `ManagedBinaries` to the app: runs the launch check + 1 Hz
/// tick loop, drains its notification queue, and republishes a
/// `ManagedBinariesStatus` for the Settings "Components" section. (The Rust
/// core invalidates every engine/grabber binary lookup itself when the
/// binaries change.)
@MainActor
@Observable
final class ManagedBinariesController {
    private let managed: ManagedBinaries
    private var tickTask: Task<Void, Never>?
    private let notify: @MainActor (String) -> Void
    private nonisolated static let log = Logger(
        subsystem: "com.shayanoh.SDM", category: "binaries")
    /// Called when the installed ffmpeg or yt-dlp may have changed, so the
    /// link grabber can refresh its "ffmpeg is available" flag (the Rust
    /// session has no way to notice by itself).
    @ObservationIgnored var onBinariesMayHaveChanged: (() -> Void)?

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

    init(notify: @escaping @MainActor (String) -> Void = { _ in }) {
        self.notify = notify
        let config = ManagedBinariesConfig(
            binDirectory: Self.binDirectory,
            vendorAssets: Self.bundledVendorAssets(),
            channel: MediaSitesSettingsStore.ytDlpChannel)
        do {
            managed = try ManagedBinaries(config: config)
        } catch {
            fatalError("SDM could not set up its tools: \(error.localizedDescription)")
        }
    }

    func start() {
        Task { [managed] in
            do {
                try await managed.provisionBundledIfNeeded()
            } catch {
                Self.log.error(
                    "Provisioning bundled tools failed: \(error.localizedDescription, privacy: .public)"
                )
            }
            do {
                _ = try await managed.checkNow(reason: .launch)
            } catch {
                Self.log.error(
                    "Launch check failed: \(error.localizedDescription, privacy: .public)")
            }
            self.refresh()
            // The bundled ffmpeg/qjs are in place by now, whatever the
            // version strings said before.
            self.onBinariesMayHaveChanged?()
        }
        tickTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                do {
                    try await self.managed.tick()
                } catch {
                    Self.log.error(
                        "Tool update tick failed: \(error.localizedDescription, privacy: .public)")
                }
                self.refresh()
            }
        }
    }

    /// Reads the status and the queued banners. A failed read keeps the
    /// previous status; the next call tries again.
    private func refresh() {
        do {
            let newStatus = try managed.status()
            if newStatus != status {
                let toolsChanged =
                    newStatus.ffmpegVersion != status.ffmpegVersion
                    || newStatus.ytDlpVersion != status.ytDlpVersion
                status = newStatus
                if toolsChanged { onBinariesMayHaveChanged?() }
            }
            for message in try managed.takeNotifications() { notify(message) }
        } catch {
            Self.log.error("Tool status failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Settings "Check now" / channel change.
    func checkNow() async {
        // The Swift core read the channel through a closure at check time.
        do {
            try managed.setChannel(channel: MediaSitesSettingsStore.ytDlpChannel)
            _ = try await managed.checkNow(reason: .manual)
        } catch {
            Self.log.error("Manual check failed: \(error.localizedDescription, privacy: .public)")
        }
        refresh()
    }

    /// Fire-and-forget: the grabber is about to resolve links and may need
    /// yt-dlp present. Coalesces with any in-flight check.
    func kick() {
        Task {
            _ = try? await managed.checkNow(reason: .resolveNeeded)
            refresh()
        }
    }

    /// The bundle flattens `SDM/Resources/vendor/` into `Contents/Resources/`
    /// (Xcode synchronized groups don't preserve subfolders), so each file is
    /// looked up by name at the resource root.
    nonisolated private static func bundledVendorAssets() -> [VendorAsset] {
        guard
            let manifestURL = Bundle.main.url(
                forResource: "vendor-manifest", withExtension: "json"),
            let data = try? Data(contentsOf: manifestURL),
            let versions = try? JSONDecoder().decode([String: String].self, from: data)
        else { return [] }

        #if arch(arm64)
            let arch = "arm64"
        #else
            let arch = "x86_64"
        #endif

        var assets: [VendorAsset] = []
        if let v = versions["ffmpegVersion"], v != "0" {
            if let url = Bundle.main.url(forResource: "ffmpeg-\(arch)", withExtension: "lzfse") {
                assets.append(VendorAsset(name: "ffmpeg", compressedURL: url, version: v))
            }
            // ffprobe ships from the same build; yt-dlp's HLS/DASH fixup
            // postprocessors need it.
            if let url = Bundle.main.url(forResource: "ffprobe-\(arch)", withExtension: "lzfse") {
                assets.append(VendorAsset(name: "ffprobe", compressedURL: url, version: v))
            }
        }
        if let v = versions["qjsVersion"], v != "0",
            let url = Bundle.main.url(forResource: "qjs", withExtension: "lzfse")
        {
            assets.append(VendorAsset(name: "qjs", compressedURL: url, version: v))
        }
        return assets
    }
}
