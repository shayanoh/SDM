import SDMCore
import SDMGrabber
import SwiftUI
import UniformTypeIdentifiers

struct LinkGrabberView: View {
    @Environment(GrabberController.self) private var controller
    @Environment(EngineController.self) private var engineController
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
    @State private var activeFilter: VerdictFilter = .all
    @State private var isShowingAddSheet = false
    @State private var isShowingRenameAlert = false
    @State private var renamingPackage = ""
    @State private var newPackageName = ""

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            List {
                ForEach(controller.snapshot.packages, id: \.name) { package in
                    Section {
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller, theme: theme)
                                .draggable(DraggedLinkID(linkID: link.id))
                        }
                    } header: {
                        packageHeader(package)
                    }
                }
            }
            // `List` paints its own opaque system background regardless of
            // what sits behind it — without hiding that, `surfacePrimary`
            // never actually shows through.
            .scrollContentBackground(.hidden)
            .background(theme.surfacePrimaryColor)
        }
        .alert("Rename package", isPresented: $isShowingRenameAlert) {
            TextField("Name", text: $newPackageName)
            Button("Rename") {
                let old = renamingPackage
                let new = newPackageName
                Task { await controller.renamePackage(old, to: new) }
            }
            Button("Cancel", role: .cancel) {}
        }
        .onDrop(of: [.url, .plainText], isTargeted: nil) { providers in
            for provider in providers {
                _ = provider.loadObject(ofClass: NSString.self) { object, _ in
                    guard let text = object as? String else { return }
                    Task { @MainActor in await controller.ingest(text: text) }
                }
            }
            return true
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Add links") { isShowingAddSheet = true }
            }
        }
        .sheet(isPresented: $isShowingAddSheet) {
            AddLinksSheet()
        }
        .frame(minWidth: 640, minHeight: 420)
    }

    @ViewBuilder
    private func packageHeader(_ package: PackageCandidate) -> some View {
        HStack {
            Text(package.name)
            Spacer()
            Button("Add to downloads") {
                addToDownloads(package, startImmediately: false)
            }
            .controlSize(.small)
            Button("Add and start") {
                addToDownloads(package, startImmediately: true)
            }
            .controlSize(.small)
        }
        .dropDestination(for: DraggedLinkID.self) { dragged, _ in
            guard let dragged = dragged.first else { return false }
            let name = package.name
            Task { await controller.moveLink(dragged.linkID, toPackageNamed: name) }
            return true
        }
        .contextMenu {
            Button("Rename…") {
                renamingPackage = package.name
                newPackageName = package.name
                isShowingRenameAlert = true
            }
            Menu("Merge into") {
                ForEach(
                    controller.snapshot.packages.filter { $0.name != package.name }, id: \.name
                ) { other in
                    Button(other.name) {
                        let source = package.name
                        let destination = other.name
                        Task { await controller.mergePackages(source, into: destination) }
                    }
                }
            }
            Button("Split") {
                let name = package.name
                Task { await controller.splitPackage(name) }
            }
        }
    }

    private var header: some View {
        let snapshot = controller.snapshot
        return VStack(alignment: .leading, spacing: 6) {
            ProgressView(
                value: Double(snapshot.checkedCount), total: Double(max(snapshot.totalCount, 1)))
            HStack(spacing: 12) {
                Text("\(snapshot.checkedCount) / \(snapshot.totalCount) checked")
                    .font(.caption)
                    .foregroundStyle(theme.textSecondaryColor)
                Spacer()
                filterChip(.online, count: snapshot.onlineCount)
                filterChip(.faulty, count: snapshot.faultyCount)
                filterChip(.offline, count: snapshot.offlineCount)
                filterChip(.failed, count: snapshot.failedCount)
            }
        }
        .padding()
        // Applied before `sdmSurface` so the opaque theme color is what's
        // actually visible — `sdmSurface`'s translucent material alone has
        // no theme color of its own, it just blurs whatever sits behind it.
        .background(theme.surfaceSecondaryColor)
        .sdmSurface(.toolbar)
    }

    private func filterChip(_ filter: VerdictFilter, count: Int) -> some View {
        Button {
            activeFilter = activeFilter == filter ? .all : filter
        } label: {
            Text("\(filter.label) \(count)")
                .font(.caption)
        }
        .buttonStyle(.bordered)
        .tint(activeFilter == filter ? theme.accentColor : theme.textSecondaryColor)
    }

    /// Hands the package's links to the engine, then clears them out of the
    /// grabber — once a link is a download it has no reason to still show up
    /// as something waiting to be grabbed.
    private func addToDownloads(_ package: PackageCandidate, startImmediately: Bool) {
        let urls = controller.urls(inPackageNamed: package.name)
        let name = package.name
        let linkIDs = package.linkIDs
        Task {
            await engineController.addPackage(
                name: name, urls: urls, startImmediately: startImmediately)
            for linkID in linkIDs {
                await controller.removeLink(linkID)
            }
        }
    }

    private func links(in package: PackageCandidate) -> [ProbedLink] {
        let ids = Set(package.linkIDs)
        return controller.snapshot.links.filter {
            ids.contains($0.id) && activeFilter.matches($0.verdict)
        }
    }
}

enum VerdictFilter: Equatable {
    case all, online, faulty, offline, failed

    var label: String {
        switch self {
        case .all: return "All"
        case .online: return "Online"
        case .faulty: return "Faulty"
        case .offline: return "Offline"
        case .failed: return "Check failed"
        }
    }

    func matches(_ verdict: Verdict?) -> Bool {
        switch self {
        case .all: return true
        case .online: return verdict == .online
        case .offline: return verdict == .offline
        case .failed: return verdict == .checkFailed
        case .faulty:
            if case .faulty = verdict { return true }
            return false
        }
    }
}

private struct LinkRow: View {
    let link: ProbedLink
    let controller: GrabberController
    let theme: Theme

    var body: some View {
        HStack {
            Text(link.effectiveFilename).lineLimit(1)
            if link.isDuplicate {
                Text("duplicate")
                    .font(.caption)
                    .foregroundStyle(theme.faultyColor)
            }
            Spacer()
            verdictBadge
            Button(role: .destructive) {
                let id = link.id
                Task { await controller.removeLink(id) }
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.plain)
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var verdictBadge: some View {
        switch link.verdict {
        case .online:
            Text("online").font(.caption).foregroundStyle(theme.onlineColor)
        case .offline:
            Text("offline").font(.caption).foregroundStyle(theme.offlineColor)
        case .checkFailed:
            Text("check failed").font(.caption).foregroundStyle(theme.failedColor)
        case .faulty(let reason):
            // Spec §7.3: the faulty reason *is* the badge text.
            Text(reason).font(.caption).foregroundStyle(theme.faultyColor)
        case nil:
            // No verdict yet: spec §7.5's queued → probing → sniffing → done
            // per-link state, shown literally rather than a bare spinner.
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text(stageLabel).font(.caption).foregroundStyle(theme.textSecondaryColor)
            }
        }
    }

    private var stageLabel: String {
        switch link.stage {
        case .queued: return "queued"
        case .probing: return "probing"
        case .sniffing: return "sniffing"
        case .done: return "done"
        }
    }
}
