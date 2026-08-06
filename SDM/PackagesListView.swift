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
    /// Off for the Completed tab: Pause All/Resume All toggles `isEnabled`,
    /// which has no effect on a `.completed` item.
    let showsPauseResumeButton: Bool

    @Environment(EngineController.self) private var controller
    @Binding var selectedItemIDs: Set<UUID>
    @Binding var collapsedPackageIDs: Set<UUID>
    @Binding var pendingDeletion: ContentView.PendingDeletion?

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
            isSelected: selectedItemIDs.contains(item.id)
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
        Color(nsColor: .underPageBackgroundColor)
            .opacity(index.isMultiple(of: 2) ? 0.5 : 0.9)
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
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                }
                Spacer()
                Sparkline(samples: package.bytesPerSecondHistory)
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
                    await controller.setEnabled(true, for: item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStart))
        Button(isMultiple ? "Stop All" : "Stop") {
            Task {
                for item in items where canStop(item) {
                    await controller.setEnabled(false, for: item.id)
                }
            }
        }
        .disabled(!items.contains(where: canStop))
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

    private var allEnabledStopped: Bool {
        let items = packages.flatMap(\.items)
        return !items.isEmpty && items.allSatisfy { !$0.isEnabled }
    }

    private var bottomBar: some View {
        HStack {
            if showsPauseResumeButton {
                Button {
                    Task { await controller.setAllEnabled(allEnabledStopped) }
                } label: {
                    Label(
                        allEnabledStopped ? "Resume All" : "Pause All",
                        systemImage: allEnabledStopped ? "play.fill" : "pause.fill"
                    )
                }
                Divider().frame(height: 16)
            }
            Text(formatted(packages.reduce(0) { $0 + $1.bytesPerSecond }))
                .font(.title3.monospacedDigit())
            Spacer()
            Text("\(packages.count) packages")
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}

/// "Start" (`setEnabled(true)`) only does something for an item sitting
/// stopped in the queue — a running item is already going, and a
/// completed/failed item isn't started by this call at all (failed items
/// need `Retry`, per `DownloadEngine.retry`'s own doc comment).
private func canStart(_ item: ItemSnapshot) -> Bool {
    if case .queued = item.state { return !item.isEnabled }
    return false
}

/// "Stop" (`setEnabled(false)`) only does something for an item that is
/// actually running or queued-and-enabled; a completed, failed, or
/// already-stopped item has nothing to stop.
private func canStop(_ item: ItemSnapshot) -> Bool {
    switch item.state {
    case .running: return true
    case .queued: return item.isEnabled
    default: return false
    }
}

private func isFailedItem(_ item: ItemSnapshot) -> Bool {
    if case .failed = item.state { return true }
    return false
}

private struct ItemRow: View {
    let item: ItemSnapshot
    let index: Int
    let controller: EngineController
    let isSelected: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            stateIcon
                .font(.title3)
                .frame(width: 20)
                .padding(.top, 2)
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.filename).lineLimit(1)
                    resumabilityBadge
                    if item.fileMissing {
                        Label("file missing", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                    Spacer()
                    Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Text(formatted(item.bytesPerSecond))
                        .font(.caption.monospacedDigit())
                    Sparkline(samples: item.speedHistory)
                        .frame(width: 48, height: 16)
                }
                SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0)
                    .frame(height: 6)
                HStack {
                    statusLine
                    Spacer()
                    Text(
                        "\(formattedBytes(item.completed.totalBytes)) / \(formattedBytes(item.totalBytes ?? 0))"
                    )
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 4)
        .listRowBackground(alternatingRowBackground)
    }

    /// macOS's own zebra-striping colors, so the two shades stay correct in
    /// both light and dark appearance without hand-picking a color pair.
    ///
    /// A custom `listRowBackground` paints over the List's native selection
    /// highlight, so a selected row would otherwise look identical to an
    /// unselected one — this substitutes an accent tint for the zebra shade
    /// whenever the row is selected, rather than losing the affordance.
    private var alternatingRowBackground: Color {
        guard !isSelected else { return Color.accentColor.opacity(0.35) }
        return Color(nsColor: NSColor.alternatingContentBackgroundColors[index % 2])
    }

    /// Replaces the old per-row Start/Stop button: an at-a-glance state icon,
    /// with every mutating action moved to the right-click menu.
    @ViewBuilder
    private var stateIcon: some View {
        switch item.state {
        case .running:
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(.tint)
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
        case .queued:
            // A user-disabled item and a scheduler-preempted one are both
            // `.queued` (spec §6.1 collapses them onto one axis), so the
            // enabled flag is what tells them apart here too.
            if item.isEnabled {
                Image(systemName: "clock.fill").foregroundStyle(.secondary)
            } else {
                Image(systemName: "pause.circle.fill").foregroundStyle(.secondary)
            }
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
                .foregroundStyle(isFailed ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
            if let checkpointFailure = item.checkpointFailure {
                // Not a failure of the download, but it means a crash would
                // lose everything transferred so far.
                Text(checkpointFailure)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .lineLimit(2)
            }
        }
    }

    private var isFailed: Bool {
        if case .failed = item.state { return true }
        return false
    }

    private static func describe(_ item: ItemSnapshot) -> String {
        switch item.state {
        case .queued: return item.isEnabled ? "Queued" : "Stopped"
        case .running: return "Running"
        case .completed: return "Completed"
        case .failed(let reason): return "Failed — \(reason)"
        }
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
                .foregroundStyle(.secondary)
        case true, nil:
            EmptyView()
        }
    }
}

/// Renders the completed `RangeSet` directly, rasterized to the bar's pixel
/// width so it stays correct at any segment count. See spec §9.4.
struct SegmentedProgressBar: View {
    let completed: RangeSet
    let total: Int64

    var body: some View {
        Canvas { context, size in
            let background = Path(
                roundedRect: CGRect(origin: .zero, size: size),
                cornerRadius: size.height / 2
            )
            context.fill(background, with: .color(.secondary.opacity(0.25)))

            guard total > 0 else { return }
            for range in completed.ranges {
                let x = size.width * CGFloat(range.start) / CGFloat(total)
                let width = size.width * CGFloat(range.length) / CGFloat(total)
                context.fill(
                    Path(CGRect(x: x, y: 0, width: max(width, 0.5), height: size.height)),
                    with: .color(.accentColor)
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
