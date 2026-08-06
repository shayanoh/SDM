import SDMCore
import SDMEngine
import SwiftUI

/// The one downloads-list component: collapsible per-package sections with
/// zebra striping, state icons, right-click menus, and multi-select.
///
/// Both the Downloads tab (the live list — reorderable, with the global
/// pause/resume bar) and the Completed tab (a package-filtered view of the
/// same snapshot — not reorderable, no pause/resume) render through this one
/// view rather than each having their own copy, so a change to how the list
/// looks or behaves lands in both places at once instead of the two drifting
/// apart.
struct PackagesListView: View {
    let packages: [PackageSnapshot]
    /// Off for the Completed tab: dragging a completed item to reorder it, or
    /// dropping it into another package, has no effect worth offering — item
    /// order only matters for scheduling, and a completed item isn't
    /// scheduled again. `moveItem` also only accepts `.queued` items, so the
    /// drop would silently no-op anyway; this just hides the affordance for
    /// something that cannot do anything.
    let allowsReordering: Bool
    /// Off for the Completed tab: Pause All/Resume All only touches
    /// `.queued`/`.running`/`.stopped` items, which a `.completed` item never
    /// is.
    let showsPauseResumeButton: Bool

    @Environment(EngineController.self) private var controller
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
    @Binding var selectedItemIDs: Set<UUID>
    @Binding var collapsedPackageIDs: Set<UUID>
    @Binding var pendingDeletion: MainWindowView.PendingDeletion?

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    var body: some View {
        VStack(spacing: 0) {
            list
            Divider()
            bottomBar
        }
        .background(keyboardShortcuts)
    }

    private var list: some View {
        List(selection: $selectedItemIDs) {
            ForEach(Array(packages.enumerated()), id: \.element.id) { packageIndex, package in
                DisclosureGroup(isExpanded: isExpandedBinding(package.id)) {
                    ForEach(Array(package.items.enumerated()), id: \.element.id) {
                        itemIndex, item in
                        row(item, index: itemIndex)
                    }
                    .onMove(
                        perform: allowsReordering
                            ? { indices, newOffset in
                                var ids = package.items.map(\.id)
                                ids.move(fromOffsets: indices, toOffset: newOffset)
                                let packageID = package.id
                                Task { await controller.reorderItems(ids, inPackage: packageID) }
                            } : nil
                    )
                } label: {
                    packageHeader(package, index: packageIndex)
                }
                .listRowBackground(packageHeaderBackground(index: packageIndex))
            }
            .onMove(
                perform: allowsReordering
                    ? { indices, newOffset in
                        var ids = packages.map(\.id)
                        ids.move(fromOffsets: indices, toOffset: newOffset)
                        Task { await controller.reorderPackages(ids) }
                    } : nil
            )
        }
        // `List` paints its own opaque system background regardless of what
        // sits behind it — without hiding that, `surfacePrimary` never
        // actually shows through, no matter how many rows/icons/text read
        // theme roles correctly.
        .scrollContentBackground(.hidden)
        .background(theme.surfacePrimaryColor)
        // `List`'s native selection highlight is a separate layer drawn on
        // top of `.listRowBackground`, tinted by the system control accent
        // (blue by default) — `.listRowBackground` alone can't override it.
        // `.tint(_:)` does not reach it despite looking like it should;
        // `.listItemTint(.fixed(_:))` is the actual documented API.
        .listItemTint(.fixed(theme.selectionTintColor))
    }

    @ViewBuilder
    private func row(_ item: ItemSnapshot, index: Int) -> some View {
        if allowsReordering {
            itemRow(item, index: index)
                .draggable(DraggedItemID(itemID: item.id))
        } else {
            itemRow(item, index: index)
        }
    }

    private func itemRow(_ item: ItemSnapshot, index: Int) -> some View {
        ItemRow(
            item: item, index: index, controller: controller,
            isSelected: selectedItemIDs.contains(item.id), theme: theme
        )
        .tag(item.id)
        .contextMenu {
            itemsContextMenu(selectedItemIDs.contains(item.id) ? selectedItemIDs : [item.id])
        }
    }

    /// Cmd-A (select every item currently listed here), Backspace (remove the
    /// selection from the list only), and Cmd-Backspace (remove the
    /// selection *and* trash the underlying files, Finder-style). Hidden
    /// buttons rather than `.onKeyPress`, so the shortcuts work regardless of
    /// which row currently has focus. Scoped entirely to `packages` — the
    /// Completed tab's copy of this view only ever selects/deletes completed
    /// items, never reaching into the rest of the list.
    private var keyboardShortcuts: some View {
        Group {
            Button("") {
                selectedItemIDs = Set(packages.flatMap(\.items).map(\.id))
            }
            .keyboardShortcut("a", modifiers: .command)
            .opacity(0)
            .frame(width: 0, height: 0)

            Button("") {
                guard !selectedItemIDs.isEmpty else { return }
                let ids = selectedItemIDs
                selectedItemIDs.removeAll()
                Task { await controller.removeItems(Array(ids), deleteFile: false) }
            }
            .keyboardShortcut(.delete, modifiers: [])
            .opacity(0)
            .frame(width: 0, height: 0)

            Button("") {
                guard !selectedItemIDs.isEmpty else { return }
                pendingDeletion = .items(selectedItemIDs)
            }
            .keyboardShortcut(.delete, modifiers: .command)
            .opacity(0)
            .frame(width: 0, height: 0)
        }
    }

    private func isExpandedBinding(_ packageID: UUID) -> Binding<Bool> {
        Binding(
            get: { !collapsedPackageIDs.contains(packageID) },
            set: { isExpanded in
                if isExpanded {
                    collapsedPackageIDs.remove(packageID)
                } else {
                    collapsedPackageIDs.insert(packageID)
                }
            }
        )
    }

    /// "Light/dark banding": alternates two system-adaptive backgrounds by
    /// package index, the same idiom `ItemRow`'s zebra striping uses, just
    /// one shade further from the base so a header reads as visually
    /// heavier than the rows beneath it.
    private func packageHeaderBackground(index: Int) -> Color {
        theme.surfaceSecondaryColor.opacity(index.isMultiple(of: 2) ? 0.5 : 0.9)
    }

    @ViewBuilder
    private func packageHeader(_ package: PackageSnapshot, index: Int) -> some View {
        let content =
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(package.name).font(.title3.bold())
                    Text(
                        "\(formattedBytes(package.completedBytes)) / \(formattedBytes(package.totalBytes))"
                    )
                    .font(.caption)
                    .foregroundStyle(theme.textSecondaryColor)
                    .monospacedDigit()
                }
                Spacer()
                Sparkline(samples: package.bytesPerSecondHistory, color: theme.graphStrokeColor)
                    .frame(width: 60, height: 20)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())
            .contextMenu {
                Button("Remove from List") {
                    Task { await controller.removePackage(package.id, deleteFiles: false) }
                }
                Button("Remove and Delete Files", role: .destructive) {
                    pendingDeletion = .package(package.id)
                }
            }
        if allowsReordering {
            content.dropDestination(for: DraggedItemID.self) { dragged, _ in
                guard let dragged = dragged.first else { return false }
                let packageID = package.id
                Task {
                    await controller.moveItem(dragged.itemID, toPackage: packageID)
                }
                return true
            }
        } else {
            content
        }
    }

    @ViewBuilder
    private func itemsContextMenu(_ ids: Set<UUID>) -> some View {
        let items = packages.flatMap(\.items).filter { ids.contains($0.id) }
        let isMultiple = items.count > 1
        let suffix = isMultiple ? "s" : ""
        Button(isMultiple ? "Start All" : "Start") {
            Task {
                for item in items where canStart(item) {
                    await controller.startItem(item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStart))
        Button(isMultiple ? "Stop All" : "Stop") {
            Task {
                for item in items where canStop(item) {
                    await controller.stopItem(item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStop))
        Divider()
        Button(isMultiple ? "Disable All" : "Disable") {
            Task {
                for item in items where canDisable(item) {
                    await controller.setEnabled(false, for: item.id)
                }
            }
        }
        .disabled(!items.contains(where: canDisable))
        Button(isMultiple ? "Enable All" : "Enable") {
            Task {
                for item in items where canEnable(item) {
                    await controller.setEnabled(true, for: item.id)
                }
            }
        }
        .disabled(!items.contains(where: canEnable))
        if items.contains(where: isFailedItem) {
            Button("Retry") {
                Task {
                    for item in items where isFailedItem(item) {
                        await controller.retry(item.id)
                    }
                }
            }
        }
        Button("Reset Download\(suffix)") {
            Task { for id in ids { await controller.resetDownload(id) } }
        }
        Divider()
        Button("Remove from List") {
            Task { await controller.removeItems(Array(ids), deleteFile: false) }
        }
        .keyboardShortcut(.delete, modifiers: [])
        Button("Remove and Delete File\(suffix)", role: .destructive) {
            pendingDeletion = .items(ids)
        }
        .keyboardShortcut(.delete, modifiers: .command)
    }

    /// Items the global bar's button actually acts on: `.completed`/`.failed`
    /// items are never touched by Pause All/Resume All, so they must not
    /// factor into which label the button shows either.
    private var downloadableItems: [ItemSnapshot] {
        packages.flatMap(\.items).filter {
            switch $0.state {
            case .queued, .running, .stopped: return true
            case .completed, .failed: return false
            }
        }
    }

    /// True when there is nothing left for Pause All to do — every
    /// downloadable item is already `.stopped`. Drives which label/action the
    /// single button shows, per spec: any running/queued item means "Pause
    /// All," all-stopped means "Resume All."
    private var allDownloadableStopped: Bool {
        downloadableItems.allSatisfy { $0.state == .stopped }
    }

    private var bottomBar: some View {
        HStack {
            if showsPauseResumeButton {
                Button {
                    Task {
                        if allDownloadableStopped {
                            await controller.resumeAll()
                        } else {
                            await controller.pauseAll()
                        }
                    }
                } label: {
                    Label(
                        allDownloadableStopped ? "Resume All" : "Pause All",
                        systemImage: allDownloadableStopped ? "play.fill" : "pause.fill"
                    )
                }
                .disabled(downloadableItems.isEmpty)
                Divider().frame(height: 16)
            }
            Text(formatted(packages.reduce(0) { $0 + $1.bytesPerSecond }))
                .font(.title3.monospacedDigit())
            Spacer()
            Text("\(packages.count) packages")
                .foregroundStyle(theme.textSecondaryColor)
        }
        .padding()
        // Applied before `sdmSurface` so the opaque theme color is what's
        // actually visible — `sdmSurface`'s translucent material alone has
        // no theme color of its own, it just blurs whatever sits behind it.
        .background(theme.surfaceSecondaryColor)
        .sdmSurface(.toolbar)
    }
}

/// "Start" only does something for an enabled item sitting `.stopped` — a
/// disabled item cannot be started at all (re-enable it first), and a
/// running/queued/completed/failed item isn't started by this call (failed
/// items need `Retry`, per `DownloadEngine.retry`'s own doc comment).
private func canStart(_ item: ItemSnapshot) -> Bool {
    item.state == .stopped && item.isEnabled
}

/// "Stop" only does something for an item actually running or queued; a
/// completed, failed, or already-stopped item has nothing to stop.
private func canStop(_ item: ItemSnapshot) -> Bool {
    switch item.state {
    case .running, .queued: return true
    default: return false
    }
}

/// "Disable" is offered for any enabled item that isn't finished — disabling
/// a completed download has no meaning, and a failed item can still be
/// disabled to keep `Retry` from being tempting.
private func canDisable(_ item: ItemSnapshot) -> Bool {
    guard item.isEnabled else { return false }
    switch item.state {
    case .completed: return false
    default: return true
    }
}

private func canEnable(_ item: ItemSnapshot) -> Bool {
    !item.isEnabled
}

private func isFailedItem(_ item: ItemSnapshot) -> Bool {
    if case .failed = item.state { return true }
    return false
}

#Preview {
    var isEnabled = true
    var isResumable = true
    var state = ItemState.running
    var isSelected = false
    let item = ItemSnapshot(
        id: UUID(), url: URL(fileURLWithPath: ""), filename: "Filename", totalBytes: 1_000_000,
        completed: RangeSet([ByteRange(start: 10000, end: 20000)]), state: state,
        isEnabled: isEnabled, isResumable: isResumable, activeSegments: 1, configuredSegments: 3,
        bytesPerSecond: 100000, speedHistory: [100000, 90000, 80000])
    ItemRow(
        item: item, index: 1, controller: EngineController(), isSelected: isSelected,
        theme: ThemeCatalog.builtInThemes()[0])
}
private struct ItemRow: View {
    let item: ItemSnapshot
    let index: Int
    let controller: EngineController
    let isSelected: Bool
    let theme: Theme

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            stateIcon
                .font(.title3)
                .frame(width: 20)
                .padding(.top, 2)
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.filename)
                        .lineLimit(1)
                        .strikethrough(!item.isEnabled)
                        .foregroundStyle(
                            item.isEnabled
                                ? AnyShapeStyle(theme.textPrimaryColor)
                                : AnyShapeStyle(theme.textSecondaryColor))
                    resumabilityBadge
                    if item.fileMissing {
                        Label("file missing", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(theme.faultyColor)
                    }
                    Spacer()
                    Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(theme.textSecondaryColor)
                    Text(formatted(item.bytesPerSecond))
                        .font(.caption.monospacedDigit())
                    Sparkline(samples: item.speedHistory, color: theme.graphStrokeColor)
                        .frame(width: 48, height: 16)
                }
                SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0, theme: theme)
                    .frame(height: 6)
                HStack {
                    statusLine
                    Spacer()
                    Text(
                        "\(formattedBytes(item.completed.totalBytes)) / \(formattedBytes(item.totalBytes ?? 0))"
                    )
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(theme.textSecondaryColor)
                }
            }
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 4)
        .opacity(item.isEnabled ? 1.0 : 0.55)
        .listRowBackground(alternatingRowBackground)
    }

    /// Theme-driven zebra striping. `NSColor.alternatingContentBackgroundColors`
    /// (the previous source) only adapts to system light/dark — its second
    /// stripe is a near-transparent overlay tuned for the *default* system
    /// background, so against a custom theme like Dracula or Nord one of the
    /// two stripes reads as visibly wrong/missing. This alternates between
    /// the theme's own primary and secondary surface roles instead.
    private var alternatingRowBackground: Color {
        guard !isSelected else { return theme.selectionTintColor.opacity(0.35) }
        return index.isMultiple(of: 2)
            ? theme.surfacePrimaryColor : theme.surfaceSecondaryColor.opacity(0.6)
    }

    /// Replaces the old per-row Start/Stop button: an at-a-glance state icon,
    /// with every mutating action moved to the right-click menu.
    @ViewBuilder
    private var stateIcon: some View {
        switch item.state {
        case .running:
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(theme.accentColor)
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(theme.onlineColor)
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(theme.failedColor)
        case .queued:
            Image(systemName: "clock.fill").foregroundStyle(theme.textSecondaryColor)
        case .stopped:
            Image(systemName: "pause.circle.fill").foregroundStyle(theme.textSecondaryColor)
        }
    }

    /// Surfaces `item.state`, which was never rendered. `.failed(reason:)` is
    /// the one state carrying a user-actionable message, so without this a
    /// stuck item looked identical to a working one.
    @ViewBuilder
    private var statusLine: some View {
        HStack(spacing: 6) {
            Text(Self.describe(item))
                .font(.caption)
                .foregroundStyle(
                    isFailed
                        ? AnyShapeStyle(theme.failedColor) : AnyShapeStyle(theme.textSecondaryColor)
                )
            if let checkpointFailure = item.checkpointFailure {
                // Not a failure of the download, but it means a crash would
                // lose everything transferred so far.
                Text(checkpointFailure)
                    .font(.caption)
                    .foregroundStyle(theme.faultyColor)
                    .lineLimit(2)
            }
        }
    }

    private var isFailed: Bool {
        if case .failed = item.state { return true }
        return false
    }

    private static func describe(_ item: ItemSnapshot) -> String {
        let base: String
        switch item.state {
        case .queued: base = "Queued"
        case .stopped: base = "Stopped"
        case .running: base = "Running"
        case .completed: base = "Completed"
        case .failed(let reason): base = "Failed — \(reason)"
        }
        return item.isEnabled ? base : "\(base) (disabled)"
    }

    /// Renders `isResumable`'s three states honestly: `nil` (not yet probed)
    /// shows nothing rather than implying "not resumable", which is the one
    /// state that actually warrants a warning.
    @ViewBuilder
    private var resumabilityBadge: some View {
        switch item.isResumable {
        case false:
            Text("not resumable")
                .font(.caption)
                .foregroundStyle(theme.textSecondaryColor)
        case true, nil:
            EmptyView()
        }
    }
}

/// Renders the completed `RangeSet` directly, rasterized to the bar's pixel
/// width so it stays correct at any segment count. See spec §9.4. Uses
/// spec §10.1's `progressFill` role for the fill and `surfaceTertiary` for
/// the empty track.
struct SegmentedProgressBar: View {
    let completed: RangeSet
    let total: Int64
    let theme: Theme

    var body: some View {
        Canvas { context, size in
            let background = Path(
                roundedRect: CGRect(origin: .zero, size: size),
                cornerRadius: size.height / 2
            )
            context.fill(background, with: .color(theme.surfaceTertiaryColor.opacity(0.6)))

            guard total > 0 else { return }
            for range in completed.ranges {
                let x = size.width * CGFloat(range.start) / CGFloat(total)
                let width = size.width * CGFloat(range.length) / CGFloat(total)
                context.fill(
                    Path(CGRect(x: x, y: 0, width: max(width, 0.5), height: size.height)),
                    with: .color(theme.progressFillColor)
                )
            }
        }
    }
}

func formatted(_ bytesPerSecond: Double) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .binary
    return formatter.string(fromByteCount: Int64(bytesPerSecond)) + "/s"
}

func formattedBytes(_ bytes: Int64) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .binary
    return formatter.string(fromByteCount: bytes)
}
