//
//  ContentView.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

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
    @State private var urlText = ""

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
                            ItemRow(item: item, controller: controller)
                        }
                    }
                }
            }
        }
    }

    private var downloadsTab: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("https://example.com/file.bin", text: $urlText)
                    .textFieldStyle(.roundedBorder)
                Button("Add") {
                    let text = urlText
                    urlText = ""
                    Task { await controller.addDownload(urlString: text) }
                }
                .disabled(urlText.isEmpty)
            }
            .padding()

            Divider()

            List {
                ForEach(controller.snapshot.packages) { package in
                    Section {
                        ForEach(package.items) { item in
                            ItemRow(item: item, controller: controller)
                                .draggable(DraggedItemID(itemID: item.id))
                        }
                        .onMove { indices, newOffset in
                            var ids = package.items.map(\.id)
                            ids.move(fromOffsets: indices, toOffset: newOffset)
                            let packageID = package.id
                            Task { await controller.reorderItems(ids, inPackage: packageID) }
                        }
                    } header: {
                        HStack {
                            Text(package.name)
                            Spacer()
                            Sparkline(samples: package.bytesPerSecondHistory)
                                .frame(width: 48, height: 16)
                        }
                        .dropDestination(for: DraggedItemID.self) { dragged, _ in
                            guard let dragged = dragged.first else { return false }
                            let packageID = package.id
                            Task {
                                await controller.moveItem(dragged.itemID, toPackage: packageID)
                            }
                            return true
                        }
                    }
                }
            }

            Divider()

            HStack {
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.title3.monospacedDigit())
                Spacer()
                Text("\(controller.snapshot.packages.count) packages")
                    .foregroundStyle(.secondary)
            }
            .padding()
        }
    }
}

private struct ItemRow: View {
    let item: ItemSnapshot
    let controller: EngineController

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                // Spec §6.1: enable/disable and start/stop are one axis, so
                // this button *is* the enabled toggle. Without a caller,
                // `EngineController.setEnabled` — and with it the whole
                // preempt/resume path — could not be exercised by hand at all.
                Button(item.isEnabled ? "Stop" : "Start") {
                    let enabled = !item.isEnabled
                    let id = item.id
                    Task { await controller.setEnabled(enabled, for: id) }
                }
                .controlSize(.small)
                if item.remainingAttempts != nil, isFailed {
                    Button("Retry") {
                        let id = item.id
                        Task { await controller.retry(id) }
                    }
                    .controlSize(.small)
                }
                Text(item.filename).lineLimit(1)
                resumabilityBadge
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
            statusLine
        }
        .padding(.vertical, 2)
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
        // A user-disabled item and a scheduler-preempted one are both
        // `.queued` (spec §6.1 collapses them onto one axis), so the enabled
        // flag is what tells them apart in the UI.
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

private func formatted(_ bytesPerSecond: Double) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .binary
    return formatter.string(fromByteCount: Int64(bytesPerSecond)) + "/s"
}
