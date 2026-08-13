//
//  ChromeNativeHostInstaller.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/12/26.
//

import AppKit
import Foundation
import SDMCore
import SwiftUI

enum ChromeExtension {
    static var directoryURL: URL {
        Bundle.main.resourceURL!
            .appendingPathComponent(
                "SDMChromeExtension",
                isDirectory: true
            )
    }

    static func openChromeExtensions() {
        guard
            let chromeURL = NSWorkspace.shared.urlForApplication(
                withBundleIdentifier: "com.google.Chrome"
            ),
            let extensionsURL = URL(
                string: "chrome://extensions/"
            )
        else {
            return
        }

        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.arguments = ["--new-window"]

        NSWorkspace.shared.open(
            [extensionsURL],
            withApplicationAt: chromeURL,
            configuration: configuration
        )
    }

    static func isChromeInstalled() -> Bool {
        guard
            NSWorkspace.shared.urlForApplication(
                withBundleIdentifier: "com.google.Chrome"
            ) != nil
        else {
            return false
        }
        return true
    }

    struct ChromeExtensionStatus: Decodable {
        let version: String
        let lastSeen: Date
    }

    static func chromeExtensionStatus() -> ChromeExtensionStatus? {
        guard
            let applicationSupport = FileManager.default.urls(
                for: .applicationSupportDirectory,
                in: .userDomainMask
            ).first
        else {
            return nil
        }

        let url =
            applicationSupport
            .appendingPathComponent("SDM", isDirectory: true)
            .appendingPathComponent("ChromeExtensionStatus.json")

        guard
            let data = try? Data(contentsOf: url),
            let status = try? JSONDecoder().decode(
                ChromeExtensionStatus.self,
                from: data
            )
        else {
            return nil
        }

        return status
    }

    struct ChromeExtensionManifest: Decodable {
        let version: String
    }

    static func bundledChromeExtensionVersion() -> String? {
        guard
            let url = Bundle.main.url(
                forResource: "manifest",
                withExtension: "json",
                subdirectory: "SDMChromeExtension"
            ),
            let data = try? Data(contentsOf: url),
            let manifest = try? JSONDecoder().decode(
                ChromeExtensionManifest.self,
                from: data
            )
        else {
            return nil
        }

        return manifest.version
    }

    static func isLatestVersionInstalled() -> String? {
        guard let latestVersion = bundledChromeExtensionVersion() else { return nil }
        guard let status = chromeExtensionStatus() else { return nil }
        guard latestVersion == status.version else { return nil }
        return status.version
    }
}

enum ChromeNativeHostInstaller {
    static let hostName = "com.shayanoh.sdm.chrome_extension"

    static let extensionID = "fnnpcbmjpapeohfmpiefbocloodhlgjf"

    static let manifestFileName = "\(hostName).json"

    static func install() throws {
        let fileManager = FileManager.default

        // Find the helper inside the currently running SDM.app.
        let hostURL = Bundle.main.bundleURL
            .appendingPathComponent("Contents", isDirectory: true)
            .appendingPathComponent("Helpers", isDirectory: true)
            .appendingPathComponent("SDMNativeHost", isDirectory: false)

        guard fileManager.isExecutableFile(atPath: hostURL.path) else {
            throw InstallerError.nativeHostNotFound(hostURL.path)
        }

        // ~/Library/Application Support/Google/Chrome/NativeMessagingHosts
        guard
            let applicationSupportURL = fileManager.urls(
                for: .applicationSupportDirectory,
                in: .userDomainMask
            ).first
        else {
            throw InstallerError.applicationSupportNotFound
        }

        let hostDirectoryURL =
            applicationSupportURL
            .appendingPathComponent("Google", isDirectory: true)
            .appendingPathComponent("Chrome", isDirectory: true)
            .appendingPathComponent("NativeMessagingHosts", isDirectory: true)

        try fileManager.createDirectory(
            at: hostDirectoryURL,
            withIntermediateDirectories: true
        )

        let manifestURL =
            hostDirectoryURL
            .appendingPathComponent(manifestFileName)

        let manifest = NativeHostManifest(
            name: hostName,
            description: "Shayan's Download Manager native messaging host",
            path: hostURL.path,
            type: "stdio",
            allowedOrigins: [
                "chrome-extension://\(extensionID)/"
            ]
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

        let data = try encoder.encode(manifest)

        // Atomic means Chrome never sees a half-written JSON file.
        try data.write(
            to: manifestURL,
            options: [.atomic]
        )
    }

    // MARK: - Types

    private struct NativeHostManifest: Encodable {
        let name: String
        let description: String
        let path: String
        let type: String

        enum CodingKeys: String, CodingKey {
            case name
            case description
            case path
            case type
            case allowedOrigins = "allowed_origins"
        }

        let allowedOrigins: [String]
    }

    enum InstallerError: LocalizedError {
        case nativeHostNotFound(String)
        case applicationSupportNotFound

        var errorDescription: String? {
            switch self {
            case .nativeHostNotFound(let path):
                return "SDMNativeHost was not found at: \(path)"

            case .applicationSupportNotFound:
                return "Could not locate the user's Application Support directory."
            }
        }
    }
}

struct ChromeExtensionSetupView: View {
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.colorScheme) private var colorScheme
    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    @State private var installedVersion: String?
    @State private var isComplete = false

    private func monitorExtension() async {
        // Capture whatever version was known before this setup started.

        while !Task.isCancelled && !isComplete {
            if let version = ChromeExtension.isLatestVersionInstalled() {
                installedVersion = version
                isComplete = true

                try? await Task.sleep(for: .seconds(5))

                if !Task.isCancelled {
                    isComplete = false
                    dismiss()
                }

                return
            }

            try? await Task.sleep(for: .seconds(1))
        }
    }

    var body: some View {
        Group {
            if isComplete, let installedVersion {
                successView(version: installedVersion)
            } else {
                setupView
            }
        }
        .padding(28)
        .background(theme.surfacePrimaryColor)
        .background(AlwaysOnTop())
        .toolbarBackground(theme.surfacePrimaryColor, for: .windowToolbar)
        .toolbarBackground(.visible, for: .windowToolbar)
        .toolbarColorScheme(
            theme.isDark ? .dark : .light,
            for: .windowToolbar
        )
        .task {
            await monitorExtension()
        }
    }

    struct AlwaysOnTop: NSViewRepresentable {
        func makeNSView(context: Context) -> NSView {
            let view = NSView()
            DispatchQueue.main.async {
                view.window?.level = .floating
                view.window?.hidesOnDeactivate = false
            }
            return view
        }

        func updateNSView(_ nsView: NSView, context: Context) {}
    }

    private func successView(version: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 76))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(.green)

            Text("You're all set!")
                .font(.title2)
                .fontWeight(.semibold)

            Text("SDM Chrome Extension \(version) is installed.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(width: 360)
    }

    private var setupView: some View {
        VStack(alignment: .leading, spacing: 20) {

            Text("Install the Chrome Extension")
                .font(.title2)
                .fontWeight(.semibold)

            Text(
                "Follow these steps to enable SDM to capture downloads from Google Chrome."
            )
            .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 16) {

                StepView(
                    number: 1,
                    title: "Open Chrome's Extensions page",
                    theme: theme
                ) {
                    Button("Open Chrome Extensions") {
                        ChromeExtension.openChromeExtensions()
                    }
                    .frame(minWidth: 170)
                }

                StepView(
                    number: 2,
                    title: "Enable Developer mode",
                    theme: theme
                ) {
                    Text(
                        "In the top-right corner of the Extensions page, turn on Developer mode."
                    )
                    .foregroundStyle(.secondary)

                    // Screenshot goes here.
                    Image("Chrome-DeveloperMode")
                        .resizable()
                        .scaledToFit()
                        .frame(height: 90)
                        .frame(maxWidth: .infinity, alignment: .center)
                }

                StepView(
                    number: 3,
                    title: "Choose Load unpacked",
                    theme: theme
                ) {
                    Text(
                        "Click Load unpacked. A folder selection dialog will appear."
                    )
                    .foregroundStyle(.secondary)

                    // Screenshot goes here.
                    Image("Chrome-Load")
                        .resizable()
                        .scaledToFit()
                        .frame(height: 100)
                        .frame(maxWidth: .infinity, alignment: .center)
                }

                StepView(
                    number: 4,
                    title: "Drag the SDM extension folder into the dialog",
                    theme: theme
                ) {
                    Text(
                        "Drag this folder onto Chrome's folder selection dialog, then click Select."
                    )
                    .foregroundStyle(.secondary)

                    ChromeExtensionFolderView()
                        .frame(maxWidth: 400)
                }
                Button("I've installed the extension") {
                    isComplete = false
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .frame(maxWidth: .infinity)
            }
        }
        .padding(28)
        .frame(width: 540)
        .fixedSize(horizontal: true, vertical: true)
        .background(theme.surfacePrimaryColor)
        .toolbarBackground(theme.surfacePrimaryColor, for: .windowToolbar)
        .toolbarBackground(.visible, for: .windowToolbar)
        .toolbarColorScheme(theme.isDark ? .dark : .light, for: .windowToolbar)
        .task({

        })
    }

    struct StepView<Content: View>: View {
        let number: Int
        let title: String
        public var theme: Theme
        @ViewBuilder let content: () -> Content

        var body: some View {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text("\(number)")
                        .font(.caption)
                        .fontWeight(.bold)
                        .frame(width: 22, height: 22)
                        .background(theme.accentColor, in: Circle())
                        .foregroundStyle(theme.textPrimaryColor)

                    Text(title)
                        .fontWeight(.medium)
                }

                content()
                    .padding(.leading, 30)
            }
        }
    }

    struct ChromeExtensionFolderView: View {
        private let folderURL = ChromeExtension.directoryURL

        var body: some View {
            let icon = NSWorkspace.shared.icon(
                forFile: Bundle.main.bundlePath
            )

            HStack(spacing: 12) {
                Image(nsImage: icon)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: 48, height: 48)

                VStack(alignment: .leading, spacing: 2) {
                    Text(folderURL.lastPathComponent)
                        .font(.system(size: 14, weight: .medium))

                    Text("Chrome extension")
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                }

                Spacer()
            }
            .padding(10)
            .background {
                RoundedRectangle(cornerRadius: 8)
                    .fill(.quaternary)
            }
            .contentShape(Rectangle())
            .onDrag {
                NSItemProvider(contentsOf: folderURL)!
            }
        }
    }
}
