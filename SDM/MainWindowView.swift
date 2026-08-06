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
    @Binding var selection: SidebarItem?
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

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Label("Downloads", systemImage: "arrow.down.circle").tag(SidebarItem.downloads)
                Label("Completed", systemImage: "checkmark.circle").tag(SidebarItem.completed)
                Label("Linkgrabber", systemImage: "link")
                    .tag(SidebarItem.linkgrabber)
                    .badge(grabberController.snapshot.totalCount)
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
            Task { await controller.removeItems(Array(ids), deleteFile: true) }
        case .package(let id):
            Task { await controller.removePackage(id, deleteFiles: true) }
        }
    }

    private var statsBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            if activeCount == 0 {
                Text("No running downloads")
                    .font(.headline)
                    .foregroundStyle(.secondary)
            } else {
                Text(formatted(controller.snapshot.globalBytesPerSecond)).font(
                    .headline.monospacedDigit())
            }
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }

    private var activeCount: Int {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }.count
    }

    private var downloadsTab: some View {
        PackagesListView(
            packages: controller.snapshot.packages,
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
        controller.snapshot.packages.compactMap { package in
            let completedItems = package.items.filter { $0.state == .completed }
            guard !completedItems.isEmpty else { return nil }
            return PackageSnapshot(
                id: package.id, name: package.name, priority: package.priority,
                items: completedItems)
        }
    }
}

struct DeletionInfo {
    let title: String
    let totalBytes: Int64
    let names: [String]

    var count: Int { names.count }
}

#Preview {
    let delInfo = DeletionInfo(title: "Delete files?", totalBytes: 1_000_000_000, names: ["file1","file2"])
    let delInfo2 = DeletionInfo(title: "Delete files?", totalBytes: 1_000_000_000, names: [])
    VStack(spacing:20) {
        DeletionConfirmationView(info: delInfo, onCancel: { }, onDelete: { })
        DeletionConfirmationView(info: delInfo2, onCancel: { }, onDelete: { })
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
