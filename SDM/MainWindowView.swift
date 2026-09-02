//
//  ContentView.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import AppKit
import SDMCore
import SDMEngine
import SDMGrabber
import SwiftUI

struct MainWindowView: View {
    enum SidebarItem: Hashable {
        case downloads, linkgrabber, completed
    }

    @Environment(EngineController.self) private var controller
    @Environment(GrabberController.self) private var grabberController
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow
    @Environment(\.scenePhase) private var scenePhase
    @Binding var selection: SidebarItem?
    private var theme: Theme { themeStore.resolved(for: colorScheme) }
    @State private var selectedItemIDs: Set<UUID> = []
    @State private var selectedCompletedItemIDs: Set<UUID> = []
    @State private var collapsedPackageIDs: Set<UUID> = []
    @State private var collapsedCompletedPackageIDs: Set<UUID> = []
    @State private var pendingDeletion: PendingDeletion?

    /// What a "Remove and Delete" confirmation is about to act on. Unified
    /// into one enum (rather than two separate optional-ID states, one per
    /// kind) so there is exactly one confirmation surface in the view — two
    /// independent `.confirmationDialog`/`.sheet` modifiers stacked on the
    /// same view is a known source of "the second one silently never
    /// presents" bugs, which is what made the package-delete confirmation
    /// unreliable before this.
    enum PendingDeletion: Identifiable {
        case items(Set<UUID>)
        case package(UUID)

        var id: String {
            switch self {
            case .items(let ids): return "items-\(ids.sorted().map(\.uuidString).joined())"
            case .package(let id): return "package-\(id.uuidString)"
            }
        }
    }

    private var overallFraction: Double {
        let running = controller.snapshot.packages.flatMap(\.items).filter {
            ($0.state == .running || $0.state == .queued || $0.state == .completed)
                && ($0.totalBytes ?? 0) > 0
        }
        guard !running.isEmpty else { return 0 }
        return running.reduce(0.0) { $0 + $1.fractionCompleted } / Double(running.count)
    }

    var body: some View {
        // A `Window(id:)` scene keeps this view's state alive across
        // close/reopen so `openWindow(id:)` restores it instantly — but
        // that also means SwiftUI keeps evaluating and redrawing the
        // closed window's content (list rows, their context menus, drag
        // payloads, and the per-row `Canvas` graphs with the display
        // links backing them) even with no window left to show it.
        // `scenePhase` goes `.background` for this window once it's no
        // longer visible; swapping in an empty view then lets SwiftUI
        // tear the real content down instead of continuing to drive it,
        // and rebuilds it fresh the moment the window reopens.
        if scenePhase == .background {
            Color.clear
        } else {
            realBody
        }
    }

    private var realBody: some View {
        NavigationSplitView {
            List(selection: $selection) {
                let items = controller.snapshot.packages.flatMap(\.items)
                let countRunning = items.count(where: { i in i.state == .running })
                let countQueued = items.count(where: { i in i.state == .queued })
                let isDownloading = countRunning > 0
                HStack {
                    Label("Downloads", systemImage: "arrow.down.circle")
                        .symbolEffect(
                            .breathe, options: .repeat(.continuous), isActive: isDownloading)
                    Spacer()
                    if isDownloading {
                        Text("\(countRunning)/\(countRunning+countQueued)")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundStyle(theme.textPrimaryColor)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(theme.onlineColor)
                            .clipShape(Capsule())
                        ProgressView(value: overallFraction)
                            .progressViewStyle(.circular)
                            .tint(theme.accentColor)
                            .controlSize(.mini)
                    }
                }
                .tag(SidebarItem.downloads)
                .listRowBackground(sidebarRowBackground(.downloads))
                Label("Completed", systemImage: "checkmark.circle")
                    .tag(SidebarItem.completed)
                    .listRowBackground(sidebarRowBackground(.completed))
                // `.tag()` must be the outermost modifier — a `List`'s
                // selection binding reads a row's tag off the final composed
                // view, and `.badge()` applied *after* `.tag()` was breaking
                // that association entirely, making this row unselectable.
                Label("Linkgrabber", systemImage: "link")
                    .badge(grabberController.snapshot.totalCount)
                    .tag(SidebarItem.linkgrabber)
                    .listRowBackground(sidebarRowBackground(.linkgrabber))
                Section("Overview") { OverviewStatsBlock() }
            }
            .navigationSplitViewColumnWidth(min: 200, ideal: 220)
            // `List` paints its own opaque system background regardless of
            // what sits behind it — without hiding that, `sdmSurface`'s
            // material (and the theme's own sidebarBackground role) never
            // actually shows through.
            .scrollContentBackground(.hidden)
            .background(theme.sidebarBackgroundColor)
            // Neither `.tint(_:)` nor `.listItemTint(_:)` reaches a List's
            // native (system-accent-blue) row selection highlight — see
            // `NativeSelectionHighlightDisabler`'s doc comment. The
            // `.listRowBackground` calls above are the actual selection
            // indicator now.
            .sdmSurface(.sidebar)
        } detail: {
            switch selection ?? .downloads {
            case .downloads: downloadsTab
            case .linkgrabber: LinkGrabberView()
            case .completed: completedTab
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        // `.toolbarBackground`/`.toolbarColorScheme` are SwiftUI-managed —
        // unlike setting `NSWindow.titlebarAppearsTransparent` directly
        // (tried and reverted), SwiftUI keeps its own safe-area boundary for
        // the titlebar/toolbar region intact, so a scrolled `List` row still
        // stops at that boundary instead of painting through the title text.
        .toolbarBackground(theme.surfacePrimaryColor, for: .windowToolbar)
        .toolbarBackground(.visible, for: .windowToolbar)
        .toolbarColorScheme(theme.isDark ? .dark : .light, for: .windowToolbar)
        .task(id: themeStore.selectedID) { applyNativeAppearance() }
        .onChange(of: colorScheme) { _, _ in applyNativeAppearance() }
        // Watches `structuralPackages` rather than the tick-frequency
        // `snapshot` — evaluating this modifier reads its watched value
        // during `body`'s own evaluation, so watching the always-changing
        // `snapshot` was forcing `MainWindowView.body` (and everything it
        // constructs, including `downloadsTab`/`PackagesListView`) to
        // re-evaluate on every heartbeat tick regardless of `PackagesListView`
        // 's own optimizations. Known-download URLs only ever change on a
        // structural event (an item added or removed) anyway, so this is
        // also the semantically correct thing to watch, not just the
        // cheaper one.
        .onChange(of: controller.structuralPackages) { _, newPackages in
            let urls = Set(newPackages.flatMap { $0.items.map(\.url) })
            Task { await grabberController.setKnownDownloadURLs(urls) }
        }
        .sheet(item: $pendingDeletion) { deletion in
            DeletionConfirmationView(
                info: deletionInfo(for: deletion),
                onCancel: { pendingDeletion = nil },
                onDelete: {
                    performDeletion(deletion)
                    pendingDeletion = nil
                }
            )
        }
        .onAppear {
            Task {
                try? await Task.sleep(for: .seconds(2))
                if ChromeExtension.isChromeInstalled() {
                    if ChromeExtension.isLatestVersionInstalled() == nil,
                        let bundleVersion = ChromeExtension.bundledChromeExtensionVersion()
                    {
                        if bundleVersion != EngineSettingsStore.chromeSetupDialogLastVersion {
                            EngineSettingsStore.chromeSetupDialogLastVersion = bundleVersion
                            EngineSettingsStore.chromeSetupDialogShowCount = 0
                        }
                        if EngineSettingsStore.chromeSetupDialogShowCount < 3 {
                            EngineSettingsStore.chromeSetupDialogShowCount += 1
                            openWindow(id: "chrome-extension-setup")
                        }
                    }
                }
            }
        }
    }

    /// Spec §10.1: "Each theme declares whether it is dark, so
    /// `NSApp.appearance` is set correctly for native controls." `nil`
    /// (System) leaves `NSApp.appearance` unset so native chrome simply
    /// follows the OS; any fixed theme forces `NSApp.appearance` to match
    /// its own `isDark`, overriding the system setting. This keeps native
    /// controls (text, buttons, the traffic-light glyphs) legible against
    /// the theme — the titlebar's actual fill color comes from
    /// `.toolbarBackground(_:for: .windowToolbar)` above, not from this: a
    /// `NavigationSplitView`'s titlebar and toolbar are one merged region,
    /// so that call paints both at once. `titlebarAppearsTransparent` was
    /// tried and reverted separately: it merges the titlebar into the
    /// content view's coordinate space, so a scrolled `List` row paints
    /// straight through the window title text as it scrolls past.
    private func applyNativeAppearance() {
        NSApp.appearance =
            themeStore.selectedID == ThemeStore.systemSelectionID
            ? nil : NSAppearance(named: theme.isDark ? .darkAqua : .aqua)
    }

    /// The sidebar's own selection indicator now that its native highlight
    /// is disabled (see `hidesNativeSelectionHighlight()`) — without this,
    /// disabling the native highlight would leave the selected sidebar row
    /// with no visual indicator at all.
    private func sidebarRowBackground(_ item: SidebarItem) -> Color {
        selection == item ? theme.selectionTintColor : Color.clear
    }

    /// Gathers what the confirmation sheet needs to show — how many files,
    /// their combined size, and their names — fresh from the current
    /// snapshot each time a deletion is pending.
    private func deletionInfo(for deletion: PendingDeletion) -> DeletionInfo {
        let baseItems: [ItemSnapshot]
        switch deletion {
        case .items(let ids):
            baseItems = controller.snapshot.packages.flatMap(\.items).filter { ids.contains($0.id) }

        case .package(let id):
            guard let package = controller.snapshot.packages.first(where: { $0.id == id }) else {
                return DeletionInfo(title: "Delete Package From Disk?", totalBytes: 0, names: [])
            }
            baseItems = package.items
        }
        // Walk every artefact each item can leave on disk — the output file
        // and, for a multi-component (muxed) item, each component's part
        // file — plus their `.incomplete` sparse files. Sum the *actual*
        // on-disk sizes and list the *actual* filenames; `item.totalBytes`
        // is only an estimate and `item.filename` is just the output name.
        var totalBytes: Int64 = 0
        var names: [String] = []
        for item in baseItems {
            guard
                let package = controller.snapshot.packages.first(where: {
                    $0.items.contains { $0.id == item.id }
                })
            else { continue }
            let folder = controller.destinationPackageUrl(for: package)
            var bases = Set([item.filename])
            bases.formUnion(item.partFilenames)
            for base in bases {
                let final = folder.appendingPathComponent(base)
                for url in [final, final.appendingPathExtension("incomplete")] {
                    guard
                        let attributes = try? FileManager.default.attributesOfItem(
                            atPath: url.path),
                        let size = attributes[.size] as? Int64
                    else { continue }
                    totalBytes += size
                    names.append(url.lastPathComponent)
                }
            }
        }

        return DeletionInfo(
            title: names.isEmpty ? "Remove From List?" : "Remove From List And Delete Files?",
            totalBytes: totalBytes,
            names: names.sorted()
        )
    }

    private func performDeletion(_ deletion: PendingDeletion) {
        switch deletion {
        case .items(let ids):
            Task { await controller.removeItems(Array(ids), deleteFile: true) }
        case .package(let id):
            Task { await controller.removePackage(id, deleteFiles: true) }
        }
    }

    private var downloadsTab: some View {
        PackagesListView(
            packages: controller.structuralPackages,
            allowsReordering: true,
            showsPauseResumeButton: true,
            selectedItemIDs: $selectedItemIDs,
            collapsedPackageIDs: $collapsedPackageIDs,
            pendingDeletion: $pendingDeletion
        )
    }

    /// Spec §9.1: "Completed is a filtered view of the same list, not a
    /// separate store." Renders through the exact same `PackagesListView` the
    /// downloads tab uses — just handed a package list pre-filtered to
    /// `.completed` items — so the two tabs cannot visually or behaviorally
    /// drift apart; a change to the list component lands in both at once.
    private var completedTab: some View {
        PackagesListView(
            packages: completedPackages,
            allowsReordering: false,
            showsPauseResumeButton: false,
            selectedItemIDs: $selectedCompletedItemIDs,
            collapsedPackageIDs: $collapsedCompletedPackageIDs,
            pendingDeletion: $pendingDeletion
        )
    }

    private var completedPackages: [PackageSnapshot] {
        controller.structuralPackages.compactMap { package in
            let completedItems = package.items.filter { $0.state == .completed }
            guard !completedItems.isEmpty else { return nil }
            return PackageSnapshot(
                id: package.id, name: package.name, priority: package.priority,
                items: completedItems)
        }
    }
}

/// The sidebar's live global-speed readout and bandwidth graph. A distinct
/// `View` reading its own environment — same reasoning as
/// `PackagesListView`'s `PackageHeaderRow`/`PackagesBottomBar`: this needs
/// `controller.snapshot` at the full tick rate, but `MainWindowView.body`
/// must not, or every tick would force it to reconstruct `downloadsTab`
/// (and hence `PackagesListView`'s `List`) regardless of how stable
/// `structuralPackages` itself stays.
private struct OverviewStatsBlock: View {
    @Environment(EngineController.self) private var controller
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    private var activeCount: Int {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }.count
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if activeCount == 0 {
                Text("No running downloads")
                    .font(.headline)
                    .foregroundStyle(theme.textSecondaryColor)
            } else {
                Text(formatted(controller.snapshot.globalBytesPerSecond)).font(
                    .headline.monospacedDigit())
            }
            BandwidthGraph(
                history: controller.snapshot.globalHistory, strokeColor: theme.graphStrokeColor,
                averageStrokeColor: theme.graphAverageStrokeColor
            )
            .frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(theme.textSecondaryColor)
        }
        .padding(.vertical, 4)
    }
}

struct DeletionInfo {
    let title: String
    let totalBytes: Int64
    let names: [String]

    var count: Int { names.count }
}

#Preview {
    let delInfo = DeletionInfo(
        title: "Delete files?", totalBytes: 1_000_000_000, names: ["file1", "file2"])
    let delInfo2 = DeletionInfo(title: "Delete files?", totalBytes: 1_000_000_000, names: [])
    VStack(spacing: 20) {
        DeletionConfirmationView(info: delInfo, onCancel: {}, onDelete: {})
        DeletionConfirmationView(info: delInfo2, onCancel: {}, onDelete: {})
    }
}
/// A deliberately larger, more informative stand-in for the old one-line
/// `confirmationDialog`: shows exactly how many files and how many bytes are
/// about to be trashed, plus their names, rather than asking the operator to
/// trust a generic "Delete this file?" prompt.
struct DeletionConfirmationView: View {
    let info: DeletionInfo
    let onCancel: () -> Void
    let onDelete: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "trash.fill")
                    .font(.system(size: 34))
                    .foregroundStyle(.red)
                VStack(alignment: .leading, spacing: 4) {
                    Text(info.title).font(.title2.bold())
                    if info.count == 0 {
                        Text("No files will be moved to the Trash")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    } else {
                        Text(
                            "\(info.count) file\(info.count == 1 ? "" : "s") · \(formattedBytes(info.totalBytes)) will be moved to the Trash."
                        )
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    }
                }
            }

            if !info.names.isEmpty {
                ScrollView {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(info.names.enumerated()), id: \.offset) { _, name in
                            Text(name)
                                .font(.callout)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                }
                .frame(maxHeight: 180)
                .background(Color(nsColor: .textBackgroundColor).opacity(0.5))
                .clipShape(RoundedRectangle(cornerRadius: 8))
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                if info.count == 0 {
                    Button("Remove from List", role: .destructive, action: onDelete)
                        .keyboardShortcut(.defaultAction)
                } else {
                    Button("Move to Trash", role: .destructive, action: onDelete)
                        .keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(24)
        .frame(minWidth: 440, maxWidth: 640)
    }
}
