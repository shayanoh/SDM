import AppKit
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
            PackagesBottomBar(
                packages: packages, showsPauseResumeButton: showsPauseResumeButton, theme: theme)
        }
        .background(keyboardShortcuts)
    }

    private var list: some View {
        List(selection: $selectedItemIDs) {
            ForEach(Array(packages.enumerated()), id: \.element.id) { packageIndex, package in
                DisclosureGroup(isExpanded: isExpandedBinding(package.id)) {
                    ForEach(Array(package.items.enumerated()), id: \.element.id) {
                        itemIndex, item in
                        row(item, index: itemIndex, packageID: package.id)
                    }
                } label: {
                    PackageHeaderRow(
                        package: package, packages: packages,
                        allowsReordering: allowsReordering, theme: theme,
                        pendingDeletion: $pendingDeletion)
                }
                // Tags the header into the same selection set item rows use
                // — `Set<UUID>` has room for both since item and package ids
                // are drawn from the same random space, and it's what lets
                // native Cmd/Shift-click multi-select a run of packages the
                // same way it already does items, which a multi-package
                // drag then relies on (see `orderedDraggedPackageIDs`).
                .tag(package.id)
                .listRowBackground(
                    packageHeaderBackground(
                        index: packageIndex, isSelected: selectedItemIDs.contains(package.id)))
            }
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

    /// Every row is both a drag source and a drop target for
    /// `DraggedItemID`, and that one mechanism handles reordering whether
    /// the drop lands in this row's own package or a different one —
    /// deliberately *not* `List`'s native `.onMove`. The two cannot coexist
    /// on the same row: on macOS, once a row also carries `.draggable`,
    /// dropping outside `.onMove`'s own section (a different package, or —
    /// it turned out — even the package header) reliably cancelled the
    /// whole gesture (`NSItemProviderErrorDomain Code=-1000 "operation was
    /// cancelled"`), confirmed live even with zero tick-driven churn in the
    /// picture. See `DraggableItemRow` for the drag/drop wiring and the
    /// insertion-line indicator that replaces the native one `.onMove` used
    /// to draw.
    @ViewBuilder
    private func row(_ item: ItemSnapshot, index: Int, packageID: UUID) -> some View {
        if allowsReordering {
            DraggableItemRow(
                itemID: item.id, packageID: packageID, index: index, packages: packages,
                controller: controller, theme: theme
            ) {
                itemRow(item, index: index)
            }
        } else {
            itemRow(item, index: index)
        }
    }

    private func itemRow(_ item: ItemSnapshot, index: Int) -> some View {
        ItemRow(
            item: item, index: index, controller: controller,
            isSelected: selectedItemIDs.contains(item.id), theme: theme
        )
        .id(item.id)
        .contextMenu {
            itemsContextMenu(selectedItemIDs.contains(item.id) ? selectedItemIDs : [item.id])
        }
        .tag(item.id)
    }

    /// The package containing `item`, looked up by scanning `packages` — item
    /// snapshots don't carry a back-reference to their package, and this list
    /// is the only place that needs one, for reconstructing a destination URL
    /// to open.
    private func package(containing item: ItemSnapshot) -> PackageSnapshot? {
        packages.first { $0.items.contains { $0.id == item.id } }
    }

    /// Opens a completed item's file with its default application. A no-op
    /// for anything else — a missing or not-yet-finished file has nothing to
    /// open — so this is safe to wire to both the Return-key shortcut and
    /// the "Open File" context-menu action without each needing its own
    /// eligibility check.
    private func openFile(_ item: ItemSnapshot) {
        guard item.state == .completed, !item.fileMissing, let package = package(containing: item)
        else { return }
        NSWorkspace.shared.open(controller.destinationURL(for: item, inPackage: package))
    }

    /// Cmd-A (select every item currently listed here), Return (open every
    /// selected completed item's file), Backspace (remove the selection from
    /// the list only), and Cmd-Backspace (remove the selection *and* trash
    /// the underlying files, Finder-style). Hidden buttons rather than
    /// `.onKeyPress`, so the shortcuts work regardless of which row currently
    /// has focus. Scoped entirely to `packages` — the Completed tab's copy of
    /// this view only ever selects/deletes/opens completed items, never
    /// reaching into the rest of the list.
    private var keyboardShortcuts: some View {
        Group {
            Button("") {
                for item in packages.flatMap(\.items) where selectedItemIDs.contains(item.id) {
                    openFile(item)
                }
            }
            .keyboardShortcut(.return, modifiers: [])
            .opacity(0)
            .frame(width: 0, height: 0)

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
    /// heavier than the rows beneath it. Selection overrides the banding
    /// with the same tint `ItemRow.alternatingRowBackground` uses, now that
    /// a package header is itself a selectable, draggable row.
    private func packageHeaderBackground(index: Int, isSelected: Bool) -> Color {
        guard !isSelected else { return theme.selectionTintColor.opacity(0.35) }
        return theme.surfaceSecondaryColor.opacity(index.isMultiple(of: 2) ? 0.5 : 0.9)
    }

    @ViewBuilder
    private func itemsContextMenu(_ ids: Set<UUID>) -> some View {
        let items = packages.flatMap(\.items).filter { ids.contains($0.id) }
        let isMultiple = items.count > 1
        let suffix = isMultiple ? "s" : ""
        if items.contains(where: canOpen) {
            Button(isMultiple ? "Open Files" : "Open File") {
                for item in items where canOpen(item) {
                    openFile(item)
                }
            }
            .keyboardShortcut(.return, modifiers: [])
        }
        Button("Copy URL\(suffix) to Clipboard") {
            let urls = items.map(\.url.absoluteString).joined(separator: "\n")
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(urls, forType: .string)
        }
        Divider()
        Button("Start Download\(suffix)") {
            Task {
                for item in items where canStart(item) {
                    await controller.startItem(item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStart))
        Button("Stop Download\(suffix)") {
            Task {
                for item in items where canStop(item) {
                    await controller.stopItem(item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStop))
        Divider()
        if items.contains(where: canEnable) {
            Button(isMultiple ? "Enable All" : "Enable") {
                Task {
                    for item in items where canEnable(item) {
                        await controller.setEnabled(true, for: item.id)
                    }
                }
            }
        }
        if items.contains(where: canDisable) {
            Button(isMultiple ? "Disable All" : "Disable") {
                Task {
                    for item in items where canDisable(item) {
                        await controller.setEnabled(false, for: item.id)
                    }
                }
            }
        }
        if items.contains(where: isFailedItem) {
            Button("Retry Failed Item\(suffix)") {
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
        .disabled(!items.contains(where: canReset))
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
/// so its structure stays stable — the live byte counts/sparkline live in
/// `PackageHeaderBytesText`/`PackageHeaderSparkline`, child views, rather
/// than being read here directly. That split matters more than it looks:
/// this label and the
/// `DisclosureGroup`'s content (the package's item rows, including whichever
/// one is mid-drag) are one compound row-group entity in `List`'s AppKit
/// backing. Reading `controller.itemTelemetry` straight in *this* body — as
/// it used to — reconstructed the whole group, including the active
/// `NSItemProvider` for a row being dragged inside it, on every tick
/// (confirmed live: dragging still cancelled with
/// `NSItemProviderErrorDomain Code=-1000` even after `ItemRow` itself was
/// split the same way, because this label was still the thing invalidating
/// the group). Pushing the reads one level deeper into the two child views
/// below keeps this body — and the group it's the label
/// for — untouched by tick-driven telemetry. See `EngineController
/// .itemTelemetry`'s doc comment for why any item's telemetry invalidates
/// every reader in the first place.
private struct PackageHeaderRow: View {
    let package: PackageSnapshot
    /// The full package list, needed only to compute where a dropped
    /// package or item batch lands — see `reorderedPackageIDs(moving:
    /// preferAfter:)`.
    let packages: [PackageSnapshot]
    let allowsReordering: Bool
    let theme: Theme
    @Binding var pendingDeletion: MainWindowView.PendingDeletion?

    @Environment(EngineController.self) private var controller
    @State private var isTargeted = false
    @State private var headerHeight: CGFloat = 1

    var body: some View {
        let content =
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(package.name).font(.title3.bold())
                    PackageHeaderBytesText(package: package, controller: controller, theme: theme)
                }
                Spacer()
                PackageHeaderSparkline(package: package, controller: controller, theme: theme)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())
            .contextMenu {
                Button("Sort Items by Name") {
                    let sortedIDs = package.items
                        .sorted {
                            $0.filename.localizedStandardCompare($1.filename) == .orderedAscending
                        }
                        .map(\.id)
                    Task { await controller.reorderItems(sortedIDs, inPackage: package.id) }
                }
                Button("Remove from List") {
                    Task { await controller.removePackage(package.id, deleteFiles: false) }
                }
                Button("Remove and Delete Files", role: .destructive) {
                    pendingDeletion = .package(package.id)
                }
            }
            .overlay {
                if isTargeted {
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(theme.accentColor, lineWidth: 2)
                }
            }
        if allowsReordering {
            // A package header is a drop target for both dragged kinds: an
            // item (moves that item into this package, appended at the end)
            // and another package (reorders packages, inserting the dragged
            // one immediately before this one) — see `DraggedRowID`'s doc
            // comment for why both go through one `.dropDestination` for one
            // unified type rather than two stacked ones.
            content
                .draggable(DraggedRowID.package(package.id))
                .dropDestination(
                    for: DraggedRowID.self,
                    action: { dragged, location in
                        // A selected item row dragged alongside a selected
                        // package header arrives here as one mixed payload —
                        // both ordering helpers return `nil` for that, so
                        // this correctly falls through to "do nothing"
                        // rather than guessing which kind the operator
                        // meant.
                        if let itemIDs = orderedDraggedItemIDs(dragged, in: packages) {
                            let packageID = package.id
                            Task { await controller.moveItems(itemIDs, toPackage: packageID) }
                            return true
                        }
                        if let draggedPackageIDs = orderedDraggedPackageIDs(dragged, in: packages) {
                            // Same "there's no row after the last one to
                            // drop on" problem as items — dropping on the
                            // bottom half of a header means "after this
                            // package" instead of "before," which is the
                            // only way to ever land a package last.
                            let preferAfter = location.y > headerHeight / 2
                            guard
                                let newOrder = reorderedPackageIDs(
                                    moving: draggedPackageIDs, preferAfter: preferAfter)
                            else { return false }
                            Task { await controller.reorderPackages(newOrder) }
                            return true
                        }
                        return false
                    },
                    isTargeted: { isTargeted = $0 }
                )
                .background(
                    GeometryReader { proxy in
                        Color.clear
                            .onAppear { headerHeight = proxy.size.height }
                            .onChange(of: proxy.size.height) { _, newHeight in
                                headerHeight = newHeight
                            }
                    }
                )
        } else {
            content
        }
    }

    /// The full package-id order after moving `draggedPackageIDs` (already
    /// in their current relative order) to sit immediately before this
    /// header's own package — or, when `preferAfter`, immediately after it.
    /// Generalizes what used to be single-package index arithmetic: remove
    /// the dragged set from a copy of the full order, find where this
    /// header's own package landed in that reduced list, and reinsert the
    /// whole dragged batch there as one contiguous run. `nil` if this
    /// header's own package is itself part of the dragged set (dropping a
    /// selection onto one of its own members is a no-op, not a move).
    private func reorderedPackageIDs(moving draggedPackageIDs: [UUID], preferAfter: Bool) -> [UUID]?
    {
        let draggedSet = Set(draggedPackageIDs)
        guard !draggedSet.contains(package.id) else { return nil }

        var remaining = packages.map(\.id).filter { !draggedSet.contains($0) }
        guard let anchorIndex = remaining.firstIndex(of: package.id) else { return nil }
        let insertionIndex = preferAfter ? anchorIndex + 1 : anchorIndex
        remaining.insert(contentsOf: draggedPackageIDs, at: insertionIndex)
        return remaining
    }
}

/// The package header's live "completed / total" bytes readout. See
/// `PackageHeaderRow.body`'s doc comment for why this must be a separate
/// leaf `View` rather than inline in `PackageHeaderRow.body`. A sibling of
/// `PackageHeaderSparkline` rather than one combined view so each keeps its
/// own spot in the `HStack`/`VStack` layout the original inline code had.
private struct PackageHeaderBytesText: View {
    let package: PackageSnapshot
    let controller: EngineController
    let theme: Theme

    var body: some View {
        Text("\(formattedBytes(liveCompletedBytes)) / \(formattedBytes(liveTotalBytes))")
            .font(.caption)
            .foregroundStyle(theme.textSecondaryColor)
            .monospacedDigit()
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
}

/// The package header's live speed sparkline. See `PackageHeaderBytesText`'s
/// doc comment.
private struct PackageHeaderSparkline: View {
    let package: PackageSnapshot
    let controller: EngineController
    let theme: Theme

    var body: some View {
        HStack {
            Text(
                formatted(
                    package.items.map {
                        controller.itemTelemetry[$0.id]?.bytesPerSecond ?? $0.bytesPerSecond
                    }
                    .reduce(0, { $0 + $1 })
                )
            )
            Sparkline(samples: liveSpeedHistory, color: theme.graphStrokeColor)
                .frame(width: 60, height: 20)
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

// MARK: - Multi-selection drag ordering

/// Extracts the item ids from a drag payload — which, when the dragged row
/// is part of a larger selection, macOS bundles into one multi-element
/// payload automatically — reordered to match how they currently appear in
/// the list (package order, then item order within it) rather than
/// whatever order the drag session happened to hand back, which is
/// unspecified and not what "move these to where I dropped them" should
/// mean. `nil` when the payload mixes items and packages together (the
/// combination is ambiguous — dropping a page of files and an entire
/// package onto one target has no obvious single meaning — so the whole
/// drop is refused rather than guessing) or contains no items at all.
private func orderedDraggedItemIDs(
    _ dragged: [DraggedRowID], in packages: [PackageSnapshot]
) -> [UUID]? {
    var itemIDs: [UUID] = []
    var sawPackage = false
    for entry in dragged {
        switch entry {
        case .item(let id): itemIDs.append(id)
        case .package: sawPackage = true
        }
    }
    guard !sawPackage, !itemIDs.isEmpty else { return nil }
    let idSet = Set(itemIDs)
    return packages.flatMap(\.items).map(\.id).filter { idSet.contains($0) }
}

/// The package-id counterpart of `orderedDraggedItemIDs`: extracts and
/// reorders the dragged package ids to match their current position in
/// `packages`, or `nil` for the same "mixed payload" / "nothing of this
/// kind" reasons.
private func orderedDraggedPackageIDs(
    _ dragged: [DraggedRowID], in packages: [PackageSnapshot]
) -> [UUID]? {
    var packageIDs: [UUID] = []
    var sawItem = false
    for entry in dragged {
        switch entry {
        case .package(let id): packageIDs.append(id)
        case .item: sawItem = true
        }
    }
    guard !sawItem, !packageIDs.isEmpty else { return nil }
    let idSet = Set(packageIDs)
    return packages.map(\.id).filter { idSet.contains($0) }
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

/// "Open File" only makes sense for a genuinely finished, still-present file.
private func canOpen(_ item: ItemSnapshot) -> Bool {
    item.state == .completed && !item.fileMissing
}

/// "Reset Download" is refused on a `.running` item: a worker is actively
/// writing to that file right now, and resetting out from under an in-flight
/// write is unsafe in the same way `DownloadEngine.moveItem` refuses to move
/// one. Every other state discards cleanly since nothing is writing to it.
private func canReset(_ item: ItemSnapshot) -> Bool {
    item.state != .running
}

// MARK: - DraggableItemRow

/// Wraps a row with the one drag/drop mechanism `PackagesListView` uses for
/// every reorder — same-package or cross-package alike. See
/// `PackagesListView.row(_:index:packageID:)`'s doc comment for why this
/// replaces `List`'s native `.onMove`.
///
/// A distinct `View` rather than a modifier chain inline in `row(_:index:
/// packageID:)` because it needs its own `@State` for `isTargeted` — driving
/// a whole-row highlight (matching `PackageHeaderRow`'s) standing in for the
/// native reorder line `.onMove` used to draw. A directional line (top or
/// bottom edge) would need to know which half of the row the cursor is over
/// to point at the right edge, and `dropDestination`'s `isTargeted` only
/// gives a `Bool`, not a live position — a whole-row highlight sidesteps
/// that by not claiming a specific edge at all, matching the "nearest edge"
/// intuition without needing to track it.
private struct DraggableItemRow<Content: View>: View {
    let itemID: UUID
    let packageID: UUID
    let index: Int
    /// The full package list, needed only to reorder a multi-item drag
    /// payload into current list order — see `orderedDraggedItemIDs`.
    let packages: [PackageSnapshot]
    let controller: EngineController
    let theme: Theme
    @ViewBuilder let content: () -> Content

    @State private var isTargeted = false
    @State private var rowHeight: CGFloat = 1

    var body: some View {
        content()
            .draggable(DraggedRowID.item(itemID))
            .dropDestination(
                for: DraggedRowID.self,
                action: { dragged, location in
                    // A selected package header dragged alongside a
                    // selected item row arrives here as one mixed payload —
                    // `orderedDraggedItemIDs` returns `nil` for that (and
                    // for a drag containing only packages, which an item
                    // row never accepts), correctly falling through to "do
                    // nothing."
                    guard let itemIDs = orderedDraggedItemIDs(dragged, in: packages) else {
                        return false
                    }
                    // Every row only ever offered "insert before me," which
                    // meant nothing could ever land after the last row in a
                    // package — there's no next row to hover for that.
                    // `location.y` (the drop's position within this row) in
                    // the bottom half means "after this row" instead
                    // (`index + 1`, which `moveItems`' `atIndex` — indexed
                    // against the *pre-move* list — resolves to "append"
                    // when this is the last row).
                    let targetIndex = location.y > rowHeight / 2 ? index + 1 : index
                    Task {
                        await controller.moveItems(
                            itemIDs, toPackage: packageID, atIndex: targetIndex)
                    }
                    return true
                },
                isTargeted: { isTargeted = $0 }
            )
            .background(
                GeometryReader { proxy in
                    Color.clear
                        .onAppear { rowHeight = proxy.size.height }
                        .onChange(of: proxy.size.height) { _, newHeight in rowHeight = newHeight }
                }
            )
            .overlay {
                if isTargeted {
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(theme.accentColor, lineWidth: 2)
                }
            }
    }
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

    List(selection: $selectedItemIDs) {
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

    /// This body deliberately never reads `controller.itemTelemetry` —
    /// `.draggable(_:)` is attached to the view this `body` produces (see
    /// `PackagesListView.row(_:index:)`), and re-evaluating *this* body on
    /// every tick was what tore down the in-flight `NSItemProvider` mid-drag
    /// (confirmed via `NSItemProviderErrorDomain Code=-1000 "operation was
    /// cancelled"`), for every row on every tick — not just the row actually
    /// being dragged, since Observation tracks `itemTelemetry` as one whole
    /// dictionary property rather than per-key, so any item's telemetry
    /// change invalidates every row that reads the dictionary at all,
    /// dragged or not. All telemetry-dependent rendering is pushed one level
    /// deeper into `ItemTelemetryFields`/`ItemProgressBar`/`ItemBytesText`
    /// below, which read `controller.itemTelemetry` in their own `body`
    /// instead — so only those leaf children invalidate on a tick, and this
    /// row (and the drag it hosts) stays untouched regardless of whether
    /// this exact item, some other item, or nothing at all is downloading.
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
                    ItemTelemetryFields(
                        itemID: item.id, fallback: item, controller: controller, theme: theme)
                }
                ItemProgressBar(
                    itemID: item.id, fallback: item, controller: controller, theme: theme
                )
                .frame(height: 6)
                HStack {
                    statusLine
                    Spacer()
                    ItemBytesText(
                        itemID: item.id, fallback: item, controller: controller, theme: theme)
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

// MARK: - Live telemetry leaves

/// The segment count, speed readout, and sparkline. A distinct `View` so its
/// `controller.itemTelemetry` read lives in *this* `body`, not `ItemRow`'s —
/// see `ItemRow.body`'s doc comment for why that split matters for drag.
private struct ItemTelemetryFields: View {
    let itemID: UUID
    let fallback: ItemSnapshot
    let controller: EngineController
    let theme: Theme

    private var telemetry: ItemTelemetry? { controller.itemTelemetry[itemID] }
    private var activeSegments: Int { telemetry?.activeSegments ?? fallback.activeSegments }
    private var configuredSegments: Int {
        telemetry?.configuredSegments ?? fallback.configuredSegments
    }
    private var bytesPerSecond: Double { telemetry?.bytesPerSecond ?? 0 }
    private var speedHistory: [Double] { telemetry?.speedHistory ?? [] }

    var body: some View {
        HStack {
            Text("\(activeSegments)/\(configuredSegments) seg")
                .font(.caption.monospacedDigit())
                .foregroundStyle(theme.textSecondaryColor)
            Text(formatted(bytesPerSecond))
                .font(.caption.monospacedDigit())
            Sparkline(samples: speedHistory, color: theme.graphStrokeColor)
                .frame(width: 48, height: 16)
        }
    }
}

/// The segmented progress bar. See `ItemTelemetryFields`'s doc comment.
private struct ItemProgressBar: View {
    let itemID: UUID
    let fallback: ItemSnapshot
    let controller: EngineController
    let theme: Theme

    private var telemetry: ItemTelemetry? { controller.itemTelemetry[itemID] }
    private var completed: RangeSet { telemetry?.completed ?? fallback.completed }
    private var totalBytes: Int64? { telemetry?.totalBytes ?? fallback.totalBytes }

    var body: some View {
        SegmentedProgressBar(completed: completed, total: totalBytes ?? 0, theme: theme)
    }
}

/// The "completed / total" bytes readout. See `ItemTelemetryFields`'s doc
/// comment.
private struct ItemBytesText: View {
    let itemID: UUID
    let fallback: ItemSnapshot
    let controller: EngineController
    let theme: Theme

    private var telemetry: ItemTelemetry? { controller.itemTelemetry[itemID] }
    private var completed: RangeSet { telemetry?.completed ?? fallback.completed }
    private var totalBytes: Int64? { telemetry?.totalBytes ?? fallback.totalBytes }

    var body: some View {
        Text("\(formattedBytes(completed.totalBytes)) / \(formattedBytes(totalBytes ?? 0))")
            .font(.caption.monospacedDigit())
            .foregroundStyle(theme.textSecondaryColor)
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
