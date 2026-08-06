//
//  ContentView.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import AppKit
import SDMCore
import SDMEngine
import SwiftUI

struct ContentView: View {
    enum SidebarItem: Hashable {
        case downloads, linkgrabber, completed
    }

    @Environment(EngineController.self) private var controller
    @Environment(GrabberController.self) private var grabberController
    @Binding var selection: SidebarItem?
    @State private var selectedItemIDs: Set<UUID> = []
    @State private var collapsedPackageIDs: Set<UUID> = []
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

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Label("Downloads", systemImage: "arrow.down.circle").tag(SidebarItem.downloads)
                Label("Linkgrabber", systemImage: "link").tag(SidebarItem.linkgrabber)
                Label("Completed", systemImage: "checkmark.circle").tag(SidebarItem.completed)
                Section("Overview") { statsBlock }
            }
            .navigationSplitViewColumnWidth(min: 200, ideal: 220)
        } detail: {
            switch selection ?? .downloads {
            case .downloads: downloadsTab
            case .linkgrabber: LinkGrabberView()
            case .completed: completedTab
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .onChange(of: controller.snapshot) { _, newSnapshot in
            let urls = Set(newSnapshot.packages.flatMap { $0.items.map(\.url) })
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
    }

    /// Gathers what the confirmation sheet needs to show — how many files,
    /// their combined size, and their names — fresh from the current
    /// snapshot each time a deletion is pending.
    private func deletionInfo(for deletion: PendingDeletion) -> DeletionInfo {
        switch deletion {
        case .items(let ids):
            let items = controller.snapshot.packages.flatMap(\.items).filter { ids.contains($0.id) }
            let totalBytes = items.reduce(Int64(0)) {
                $0 + max($1.totalBytes ?? 0, $1.completed.totalBytes)
            }
            return DeletionInfo(
                title: items.count > 1 ? "Delete \(items.count) Files?" : "Delete File?",
                totalBytes: totalBytes,
                names: items.map(\.filename)
            )
        case .package(let id):
            guard let package = controller.snapshot.packages.first(where: { $0.id == id }) else {
                return DeletionInfo(title: "Delete Package?", totalBytes: 0, names: [])
            }
            return DeletionInfo(
                title: "Delete “\(package.name)”?",
                totalBytes: max(package.totalBytes, package.completedBytes),
                names: package.items.map(\.filename)
            )
        }
    }

    private func performDeletion(_ deletion: PendingDeletion) {
        switch deletion {
        case .items(let ids):
            selectedItemIDs.subtract(ids)
            Task { await controller.removeItems(Array(ids), deleteFile: true) }
        case .package(let id):
            Task { await controller.removePackage(id, deleteFiles: true) }
        }
    }

    private var statsBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(formatted(controller.snapshot.globalBytesPerSecond)).font(
                .headline.monospacedDigit())
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }

    private var activeCount: Int {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }.count
    }

    /// Spec §9.1: "Completed is a filtered view of the same list, not a
    /// separate store." A predicate over `controller.snapshot`, nothing more.
    private var completedTab: some View {
        List {
            ForEach(controller.snapshot.packages) { package in
                let completedItems = package.items.filter { $0.state == .completed }
                if !completedItems.isEmpty {
                    Section(package.name) {
                        ForEach(completedItems) { item in
                            ItemRow(item: item, index: 0, controller: controller, isSelected: false)
                                .contextMenu {
                                    Button("Reset Download") {
                                        Task { await controller.resetDownload(item.id) }
                                    }
                                    Divider()
                                    Button("Remove from List") {
                                        Task {
                                            await controller.removeItems(
                                                [item.id], deleteFile: false)
                                        }
                                    }
                                    Button("Remove and Delete File", role: .destructive) {
                                        pendingDeletion = .items([item.id])
                                    }
                                }
                        }
                    }
                }
            }
        }
    }

    // MARK: - Downloads tab

    private var downloadsTab: some View {
        VStack(spacing: 0) {
            packagesList
            Divider()
            bottomBar
        }
        .background(keyboardShortcuts)
    }

    /// Cmd-A (select all currently listed items), Backspace (remove the
    /// selection from the list only), and Cmd-Backspace (remove the
    /// selection *and* trash the underlying files, Finder-style). Hidden
    /// buttons rather than `.onKeyPress`, so the shortcuts work regardless of
    /// which row currently has focus.
    private var keyboardShortcuts: some View {
        Group {
            Button("") {
                selectedItemIDs = Set(controller.snapshot.packages.flatMap(\.items).map(\.id))
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

    private var packagesList: some View {
        List(selection: $selectedItemIDs) {
            ForEach(Array(controller.snapshot.packages.enumerated()), id: \.element.id) {
                packageIndex, package in
                DisclosureGroup(isExpanded: isExpandedBinding(package.id)) {
                    ForEach(Array(package.items.enumerated()), id: \.element.id) {
                        itemIndex, item in
                        ItemRow(
                            item: item, index: itemIndex, controller: controller,
                            isSelected: selectedItemIDs.contains(item.id)
                        )
                        .tag(item.id)
                        .draggable(DraggedItemID(itemID: item.id))
                    }
                    .onMove { indices, newOffset in
                        var ids = package.items.map(\.id)
                        ids.move(fromOffsets: indices, toOffset: newOffset)
                        let packageID = package.id
                        Task { await controller.reorderItems(ids, inPackage: packageID) }
                    }
                } label: {
                    packageHeader(package, index: packageIndex)
                }
                .listRowBackground(packageHeaderBackground(index: packageIndex))
            }
            .onMove { indices, newOffset in
                var ids = controller.snapshot.packages.map(\.id)
                ids.move(fromOffsets: indices, toOffset: newOffset)
                Task { await controller.reorderPackages(ids) }
            }
        }
        .contextMenu(forSelectionType: UUID.self) { ids in
            itemsContextMenu(ids.isEmpty ? selectedItemIDs : ids)
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

    /// Spec-adjacent "light/dark banding": alternates two system-adaptive
    /// backgrounds by package index, the same idiom `alternatingRowBackground`
    /// uses for items, just one shade further from the base so a header reads
    /// as visually heavier than the rows beneath it.
    private func packageHeaderBackground(index: Int) -> Color {
        Color(nsColor: .underPageBackgroundColor)
            .opacity(index.isMultiple(of: 2) ? 0.5 : 0.9)
    }

    @ViewBuilder
    private func packageHeader(_ package: PackageSnapshot, index: Int) -> some View {
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
        .dropDestination(for: DraggedItemID.self) { dragged, _ in
            guard let dragged = dragged.first else { return false }
            let packageID = package.id
            Task {
                await controller.moveItem(dragged.itemID, toPackage: packageID)
            }
            return true
        }
        .contextMenu {
            Button("Remove from List") {
                Task { await controller.removePackage(package.id, deleteFiles: false) }
            }
            Button("Remove and Delete Files", role: .destructive) {
                pendingDeletion = .package(package.id)
            }
        }
    }

    @ViewBuilder
    private func itemsContextMenu(_ ids: Set<UUID>) -> some View {
        let items = controller.snapshot.packages.flatMap(\.items).filter { ids.contains($0.id) }
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
        if items.contains(where: isFailed) {
            Button("Retry") {
                Task {
                    for item in items where isFailed(item) {
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
            selectedItemIDs.subtract(ids)
            Task { await controller.removeItems(Array(ids), deleteFile: false) }
        }
        .keyboardShortcut(.delete, modifiers: [])
        Button("Remove and Delete File\(suffix)", role: .destructive) {
            pendingDeletion = .items(ids)
        }
        .keyboardShortcut(.delete, modifiers: .command)
    }

    private func isFailed(_ item: ItemSnapshot) -> Bool {
        if case .failed = item.state { return true }
        return false
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

    private var allEnabledStopped: Bool {
        let items = controller.snapshot.packages.flatMap(\.items)
        return !items.isEmpty && items.allSatisfy { !$0.isEnabled }
    }

    private var bottomBar: some View {
        HStack {
            Button {
                Task { await controller.setAllEnabled(allEnabledStopped) }
            } label: {
                Label(
                    allEnabledStopped ? "Resume All" : "Pause All",
                    systemImage: allEnabledStopped ? "play.fill" : "pause.fill"
                )
            }
            Divider().frame(height: 16)
            Text(formatted(controller.snapshot.globalBytesPerSecond))
                .font(.title3.monospacedDigit())
            Spacer()
            Text("\(controller.snapshot.packages.count) packages")
                .foregroundStyle(.secondary)
        }
        .padding()
    }
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

struct DeletionInfo {
    let title: String
    let totalBytes: Int64
    let names: [String]

    var count: Int { names.count }
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
                    Text(
                        "\(info.count) file\(info.count == 1 ? "" : "s") · \(formattedBytes(info.totalBytes)) will be moved to the Trash."
                    )
                    .font(.callout)
                    .foregroundStyle(.secondary)
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
                Button("Move to Trash", role: .destructive, action: onDelete)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 440)
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
