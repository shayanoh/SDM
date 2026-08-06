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
        let _ = debugPrint("[SDM diag] PackagesListView.body evaluated")
        return VStack(spacing: 0) {
            list
            Divider()
            PackagesBottomBar(
                packages: packages, showsPauseResumeButton: showsPauseResumeButton, theme: theme)
        }
        .background(keyboardShortcuts)
    }

    private var list: some View {
        let _ = debugPrint("[SDM diag] PackagesListView.list evaluated")
        return List(selection: $selectedItemIDs) {
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
                    PackageHeaderRow(
                        package: package, allowsReordering: allowsReordering, theme: theme,
                        pendingDeletion: $pendingDeletion)
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
        // (blue by default). Neither `.tint(_:)` nor `.listItemTint(_:)`
        // reaches it — verified against current SwiftUI docs;
        // `.listItemTint` is documented as affecting only sidebar `Label`
        // icons and watchOS platters. `hidesNativeSelectionHighlight()`
        // turns the native layer off entirely so `alternatingRowBackground`
        // above is the only thing drawn.
        .tint(Color.red)
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
        .contextMenu {
            itemsContextMenu(selectedItemIDs.contains(item.id) ? selectedItemIDs : [item.id])
        }
        .tag(item.id)
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

}

// MARK: - PackageHeaderRow

/// A package's disclosure-group label. A distinct `View` (rather than a
/// helper function returning `some View`, which `packageHeader` used to be)
/// deliberately — reading `controller.itemTelemetry` inside a genuine `View`
/// struct's `body` scopes SwiftUI's invalidation to *this row alone* when
/// telemetry updates each tick, instead of forcing `PackagesListView.body` —
/// and hence the enclosing `List`'s structure — to re-evaluate. See
/// `EngineController.itemTelemetry`'s doc comment for the full story.
private struct PackageHeaderRow: View {
    let package: PackageSnapshot
    let allowsReordering: Bool
    let theme: Theme
    @Binding var pendingDeletion: MainWindowView.PendingDeletion?

    @Environment(EngineController.self) private var controller

    var body: some View {
        let _ = debugPrint("[SDM diag] PackageHeaderRow.body evaluated: \(package.name)")
        let content =
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(package.name).font(.title3.bold())
                    Text("\(formattedBytes(liveCompletedBytes)) / \(formattedBytes(liveTotalBytes))")
                        .font(.caption)
                        .foregroundStyle(theme.textSecondaryColor)
                        .monospacedDigit()
                }
                Spacer()
                Sparkline(samples: liveSpeedHistory, color: theme.graphStrokeColor)
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

    /// Live per-item telemetry summed the same way `PackageSnapshot`'s own
    /// (now-stale-on-`structuralPackages`) computed properties used to —
    /// falling back to the structural item's own value for any item
    /// telemetry hasn't reported yet (there shouldn't be a gap in practice,
    /// since `EngineController` populates `itemTelemetry` for every item on
    /// every tick, but a fallback costs nothing and avoids a silent zero).
    private var liveCompletedBytes: Int64 {
        package.items.reduce(0) {
            $0 + (controller.itemTelemetry[$1.id]?.completed.totalBytes ?? $1.completed.totalBytes)
        }
    }

    private var liveTotalBytes: Int64 {
        package.items.reduce(0) {
            $0 + (controller.itemTelemetry[$1.id]?.totalBytes ?? $1.totalBytes ?? 0)
        }
    }

    private var liveSpeedHistory: [Double] {
        let histories = package.items.map {
            controller.itemTelemetry[$0.id]?.speedHistory ?? $0.speedHistory
        }
        let length = histories.map(\.count).max() ?? 0
        guard length > 0 else { return [] }
        var summed = [Double](repeating: 0, count: length)
        for history in histories {
            let padding = length - history.count
            for (index, value) in history.enumerated() {
                summed[index + padding] += value
            }
        }
        return summed
    }
}

// MARK: - PackagesBottomBar

/// The Pause/Resume All bar and the aggregate speed readout. A distinct
/// `View` for the same reason as `PackageHeaderRow`: reading
/// `controller.itemTelemetry` here, rather than in `PackagesListView.body`
/// directly, keeps the `List` above it from re-evaluating every tick.
private struct PackagesBottomBar: View {
    let packages: [PackageSnapshot]
    let showsPauseResumeButton: Bool
    let theme: Theme

    @Environment(EngineController.self) private var controller

    var body: some View {
        let _ = debugPrint("[SDM diag] PackagesBottomBar.body evaluated")
        return HStack {
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
            Text(formatted(liveAggregateBytesPerSecond))
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

    private var liveAggregateBytesPerSecond: Double {
        packages.flatMap(\.items).reduce(0.0) {
            $0 + (controller.itemTelemetry[$1.id]?.bytesPerSecond ?? 0)
        }
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

// MARK: - ItemRow

#Preview {
    @Previewable @State var selectedItemIDs: Set<UUID> = Set<UUID>()
    let item = ItemSnapshot(
        id: UUID(),
        url: URL(fileURLWithPath: ""),
        filename: "Filename",
        totalBytes: 1_000_000,
        completed: RangeSet([ByteRange(start: 10000, end: 20000)]),
        state: .running,
        isEnabled: true,
        isResumable: true,
        activeSegments: 1,
        configuredSegments: 3,
        bytesPerSecond: 100000,
        speedHistory: [100000, 90000, 80000])
    let theme = ThemeCatalog.builtInThemes()[5]
    let controller = EngineController()
    
    List(selection:$selectedItemIDs) {
        ItemRow(
            item: item, index: 1, controller: controller, isSelected: false,
            theme: theme)
        ItemRow(
            item: item, index: 2, controller: controller, isSelected: true,
            theme: theme)
        ItemRow(
            item: item, index: 3, controller: controller, isSelected: false,
            theme: theme)
        ItemRow(
            item: item, index: 4, controller: controller, isSelected: false,
            theme: theme)
        ItemRow(
            item: item, index: 5, controller: controller, isSelected: true,
            theme: theme)
        
    }
}
private struct ItemRow: View {
    let item: ItemSnapshot
    let index: Int
    let controller: EngineController
    let isSelected: Bool
    let theme: Theme

    /// Live telemetry for this item, read directly from `controller` rather
    /// than carried on `item` — `item` comes from `EngineController
    /// .structuralPackages`, which is deliberately *not* refreshed on every
    /// tick (see its doc comment). Reading `controller.itemTelemetry` here,
    /// inside this row's own `body`, scopes SwiftUI's invalidation to just
    /// this row on a tick, instead of the whole `List` re-diffing.
    private var telemetry: ItemTelemetry? { controller.itemTelemetry[item.id] }
    private var completed: RangeSet { telemetry?.completed ?? item.completed }
    private var totalBytes: Int64? { telemetry?.totalBytes ?? item.totalBytes }
    private var activeSegments: Int { telemetry?.activeSegments ?? item.activeSegments }
    private var configuredSegments: Int { telemetry?.configuredSegments ?? item.configuredSegments }
    private var bytesPerSecond: Double { telemetry?.bytesPerSecond ?? 0 }
    private var speedHistory: [Double] { telemetry?.speedHistory ?? [] }

    var body: some View {
        let _ = debugPrint("[SDM diag] ItemRow.body evaluated: \(item.filename)")
        return HStack(alignment: .top, spacing: 8) {
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
                    Text("\(activeSegments)/\(configuredSegments) seg")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(theme.textSecondaryColor)
                    Text(formatted(bytesPerSecond))
                        .font(.caption.monospacedDigit())
                    Sparkline(samples: speedHistory, color: theme.graphStrokeColor)
                        .frame(width: 48, height: 16)
                }
                SegmentedProgressBar(completed: completed, total: totalBytes ?? 0, theme: theme)
                    .frame(height: 6)
                HStack {
                    statusLine
                    Spacer()
                    Text("\(formattedBytes(completed.totalBytes)) / \(formattedBytes(totalBytes ?? 0))")
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

// MARK: Segmented Progress Bar
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
