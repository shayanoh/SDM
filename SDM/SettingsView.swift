import SDMCore
import SDMResolve
import SwiftUI

/// Content of the dedicated `WindowGroup(id: "settings")` (see `SDMApp`) —
/// not the native `Settings {}` scene, so it can carry its own OK/Cancel
/// bar. Downloads, Linkgrabber, and Notifications are fully buffered:
/// nothing writes to a backing store until `commit()` runs on OK. The
/// Appearance tab's theme and Dock/Menu Bar picker apply live (so their
/// effect is visible behind this window immediately) and are reverted on
/// Cancel from a snapshot taken on appear.
struct SettingsView: View {
    @Environment(EngineController.self) private var controller
    @Environment(ThemeStore.self) private var themeStore
    @Environment(ActivationPolicyController.self) private var activationPolicyController
    @Environment(NotificationManager.self) private var notificationManager
    @Environment(ClipboardWatcher.self) private var clipboardWatcher
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dismiss) private var dismiss

    @State private var maxConcurrent = EngineSettingsStore.maxConcurrent
    @State private var segmentsPerItem = EngineSettingsStore.segmentsPerItem
    @State private var globalMaxConnections = EngineSettingsStore.globalMaxConnections
    @State private var maxConnectionsPerHost = EngineSettingsStore.maxConnectionsPerHost
    @State private var minSegmentSizeMB = EngineSettingsStore.minSegmentSizeMB
    @State private var autoStartDownloadsOnLaunch = EngineSettingsStore.autoStartDownloadsOnLaunch
    @State private var clipboardWatchingEnabled = GrabberSettings.clipboardWatchingEnabled
    @State private var autoAddAndStartOnGrab = GrabberSettings.autoAddAndStartOnGrab
    @State private var deepSniffEnabled = GrabberSettings.deepSniffEnabled
    @State private var autoClearInterval = GrabberSettings.autoClearGrabbedLinksAfter
    @State private var ytMaxHeight = YouTubeSettingsStore.maxHeight
    @State private var ytAllowAV1 = YouTubeSettingsStore.allowAV1
    @State private var ytAllowVP9 = YouTubeSettingsStore.allowVP9
    @State private var ytAllowH264 = YouTubeSettingsStore.allowH264
    @State private var ytAllowMP4 = YouTubeSettingsStore.allowMP4
    @State private var ytAllowWebM = YouTubeSettingsStore.allowWebM
    @State private var ytAllowOpus = YouTubeSettingsStore.allowOpus
    @State private var ytAllowAAC = YouTubeSettingsStore.allowAAC
    @State private var ytMaxPlaylistVideos = YouTubeSettingsStore.maxPlaylistVideos
    @State private var ytCookieSource = YouTubeSettingsStore.cookieSource
    @State private var downloadFinishedEnabled = NotificationSettings.downloadFinishedEnabled
    @State private var packageFinishedEnabled = NotificationSettings.packageFinishedEnabled
    @State private var downloadFailedEnabled = NotificationSettings.downloadFailedEnabled
    @State private var linksGrabbedEnabled = NotificationSettings.linksGrabbedEnabled

    @State private var originalThemeID: String?
    @State private var originalActivationMode: ActivationPolicyMode?

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    /// System, then Light and Dark, then everything else alphabetically.
    /// Presentation-only — `ThemeCatalog.builtInThemes()`'s own alphabetical
    /// order is left alone since other code (e.g. fixed preview indices)
    /// depends on it.
    private var orderedThemes: [Theme] {
        let light = themeStore.catalog.filter { $0.id == "light" }
        let dark = themeStore.catalog.filter { $0.id == "dark" }
        let rest =
            themeStore.catalog
            .filter { $0.id != "light" && $0.id != "dark" }
            .sorted { $0.name < $1.name }
        return light + dark + rest
    }

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                appearanceTab.tabItem { Label("Appearance", systemImage: "paintbrush") }
                downloadsTab.tabItem { Label("Downloads", systemImage: "arrow.down.circle") }
                linkgrabberTab.tabItem { Label("Linkgrabber", systemImage: "link") }
                youtubeTab.tabItem {
                    Label("YouTube", systemImage: "play.rectangle.on.rectangle")
                }
                notificationsTab.tabItem { Label("Notifications", systemImage: "bell") }
            }
            .padding(20)

            Divider()

            HStack {
                Spacer()
                Button("Cancel", action: cancel)
                    .keyboardShortcut(.cancelAction)
                Button("OK", action: commit)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 560)
        .fixedSize(horizontal: false, vertical: true)
        .background(theme.surfacePrimaryColor)
        // Same mechanism as `MainWindowView`: this window's titlebar and
        // its `TabView` tab strip are one merged region, so painting the
        // "toolbar" here paints the titlebar too.
        .toolbarBackground(theme.surfacePrimaryColor, for: .windowToolbar)
        .toolbarBackground(.visible, for: .windowToolbar)
        .toolbarColorScheme(theme.isDark ? .dark : .light, for: .windowToolbar)
        .onAppear {
            originalThemeID = themeStore.selectedID
            originalActivationMode = activationPolicyController.mode
        }
    }

    // MARK: Tabs

    private var appearanceTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Theme", theme: theme) {
                HStack {
                    Text("Theme")
                    Spacer()
                    Picker(
                        "",
                        selection: Binding(
                            get: { themeStore.selectedID },
                            set: { themeStore.selectedID = $0 }
                        )
                    ) {
                        Text("System").tag(ThemeStore.systemSelectionID)
                        ForEach(orderedThemes) { candidate in
                            Text(candidate.name).tag(candidate.id)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 220)
                }
            }
            SettingsSection(title: "Dock / Menu Bar", theme: theme) {
                HStack {
                    Text("Dock / Menu Bar")
                    Spacer()
                    Picker(
                        "",
                        selection: Binding(
                            get: { activationPolicyController.mode },
                            set: { activationPolicyController.mode = $0 }
                        )
                    ) {
                        ForEach(ActivationPolicyMode.allCases) { mode in
                            Text(mode.label).tag(mode)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 220)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }

    private var downloadsTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Concurrency", theme: theme) {
                Grid(alignment: .leading, verticalSpacing: 10) {
                    SteppedNumberField(
                        label: "Max concurrent downloads", value: $maxConcurrent, range: 1...20)
                    SteppedNumberField(
                        label: "Segments per file", value: $segmentsPerItem, range: 1...64)
                    SteppedNumberField(
                        label: "Global max connections", value: $globalMaxConnections,
                        range: 1...256)
                    SteppedNumberField(
                        label: "Max connections per host", value: $maxConnectionsPerHost,
                        range: 1...64)
                    SteppedNumberField(
                        label: "Minimum segment size (MB)", value: $minSegmentSizeMB,
                        range: 1...100)
                }
            }
            SettingsSection(title: "Startup", theme: theme) {
                Toggle(
                    "Resume downloads automatically when SDM opens",
                    isOn: $autoStartDownloadsOnLaunch)
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }

    private var linkgrabberTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Linkgrabber", theme: theme) {
                Toggle("Watch clipboard for links", isOn: $clipboardWatchingEnabled)
                Toggle("Auto-add and start on grab", isOn: $autoAddAndStartOnGrab)
                Toggle("Deep sniff (stage 2)", isOn: $deepSniffEnabled)
                HStack {
                    Text("Auto-clear grabbed links")
                    Spacer()
                    Picker("", selection: $autoClearInterval) {
                        ForEach(GrabberSettings.AutoClearInterval.allCases) { interval in
                            Text(interval.label).tag(interval)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 170)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }

    private var youtubeTab: some View {
        // Only one codec/container/audio may be the last one left checked —
        // `FormatSelector` must never get an empty allowlist (spec §9.4).
        let videoCount = [ytAllowAV1, ytAllowVP9, ytAllowH264].filter { $0 }.count
        let containerCount = [ytAllowMP4, ytAllowWebM].filter { $0 }.count
        let audioCount = [ytAllowOpus, ytAllowAAC].filter { $0 }.count
        return VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Quality", theme: theme) {
                HStack {
                    Text("Maximum resolution")
                    Spacer()
                    Picker("", selection: $ytMaxHeight) {
                        ForEach(YouTubeSettingsStore.resolutionChoices, id: \.self) { h in
                            Text("\(h)p").tag(h)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 120)
                }
                Grid(alignment: .leading, verticalSpacing: 10) {
                    SteppedNumberField(
                        label: "Maximum playlist videos", value: $ytMaxPlaylistVideos,
                        range: 10...200)
                }
            }
            SettingsSection(title: "Video codecs", theme: theme) {
                Toggle("AV1", isOn: $ytAllowAV1).disabled(ytAllowAV1 && videoCount == 1)
                Toggle("VP9", isOn: $ytAllowVP9).disabled(ytAllowVP9 && videoCount == 1)
                Toggle("H.264", isOn: $ytAllowH264).disabled(ytAllowH264 && videoCount == 1)
            }
            SettingsSection(title: "Containers", theme: theme) {
                Toggle("MP4", isOn: $ytAllowMP4).disabled(ytAllowMP4 && containerCount == 1)
                Toggle("WebM", isOn: $ytAllowWebM).disabled(ytAllowWebM && containerCount == 1)
            }
            SettingsSection(title: "Audio codecs", theme: theme) {
                Toggle("Opus", isOn: $ytAllowOpus).disabled(ytAllowOpus && audioCount == 1)
                Toggle("AAC", isOn: $ytAllowAAC).disabled(ytAllowAAC && audioCount == 1)
            }
            SettingsSection(title: "Authentication", theme: theme) {
                HStack {
                    Text("Cookies from browser")
                    Spacer()
                    Picker("", selection: $ytCookieSource) {
                        ForEach(CookieSource.allCases, id: \.self) { source in
                            Text(source == .none ? "None" : source.rawValue.capitalized)
                                .tag(source)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }

    private var notificationsTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Notifications", theme: theme) {
                Toggle("Download finished", isOn: $downloadFinishedEnabled)
                Toggle("Package finished", isOn: $packageFinishedEnabled)
                Toggle("Download failed", isOn: $downloadFailedEnabled)
                Toggle("Links grabbed", isOn: $linksGrabbedEnabled)
                if notificationManager.isAuthorized() != true {
                    Text(
                        "Notifications have been blocked from system settings.\nTo receive notifications enable them for SDM from Settings -> Notifications."
                    )
                    .foregroundStyle(theme.failedColor)
                    .font(.caption)
                    .multilineTextAlignment(.center)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }

    // MARK: Actions

    private func cancel() {
        if let originalThemeID {
            themeStore.selectedID = originalThemeID
        }
        if let originalActivationMode {
            activationPolicyController.mode = originalActivationMode
        }
        dismiss()
    }

    private func commit() {
        EngineSettingsStore.maxConcurrent = maxConcurrent
        EngineSettingsStore.segmentsPerItem = segmentsPerItem
        EngineSettingsStore.globalMaxConnections = globalMaxConnections
        EngineSettingsStore.maxConnectionsPerHost = maxConnectionsPerHost
        EngineSettingsStore.minSegmentSizeMB = minSegmentSizeMB
        EngineSettingsStore.autoStartDownloadsOnLaunch = autoStartDownloadsOnLaunch
        GrabberSettings.clipboardWatchingEnabled = clipboardWatchingEnabled
        // Applied live, independent of any window's visibility — see
        // `SDMApp`'s doc comment on the watcher's `.onAppear` for why it
        // must not be tied to the main window's lifecycle.
        if clipboardWatchingEnabled {
            clipboardWatcher.start()
        } else {
            clipboardWatcher.stop()
        }
        GrabberSettings.autoAddAndStartOnGrab = autoAddAndStartOnGrab
        GrabberSettings.deepSniffEnabled = deepSniffEnabled
        GrabberSettings.autoClearGrabbedLinksAfter = autoClearInterval
        YouTubeSettingsStore.maxHeight = ytMaxHeight
        YouTubeSettingsStore.allowAV1 = ytAllowAV1
        YouTubeSettingsStore.allowVP9 = ytAllowVP9
        YouTubeSettingsStore.allowH264 = ytAllowH264
        YouTubeSettingsStore.allowMP4 = ytAllowMP4
        YouTubeSettingsStore.allowWebM = ytAllowWebM
        YouTubeSettingsStore.allowOpus = ytAllowOpus
        YouTubeSettingsStore.allowAAC = ytAllowAAC
        YouTubeSettingsStore.maxPlaylistVideos = ytMaxPlaylistVideos
        YouTubeSettingsStore.cookieSource = ytCookieSource
        NotificationSettings.downloadFinishedEnabled = downloadFinishedEnabled
        NotificationSettings.packageFinishedEnabled = packageFinishedEnabled
        NotificationSettings.downloadFailedEnabled = downloadFailedEnabled
        NotificationSettings.linksGrabbedEnabled = linksGrabbedEnabled
        Task { await controller.applyStoredSettings() }
        dismiss()
    }
}
