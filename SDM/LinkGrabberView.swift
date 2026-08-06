import SDMGrabber
import SwiftUI
import UniformTypeIdentifiers

struct LinkGrabberView: View {
    @Environment(GrabberController.self) private var controller
    @Environment(EngineController.self) private var engineController
    @State private var activeFilter: VerdictFilter = .all
    @State private var isShowingAddSheet = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            List {
                ForEach(controller.snapshot.packages, id: \.name) { package in
                    Section {
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller)
                        }
                    } header: {
                        packageHeader(package)
                    }
                }
            }
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
                let urls = controller.urls(inPackageNamed: package.name)
                let name = package.name
                Task {
                    await engineController.addPackage(
                        name: name, urls: urls, startImmediately: false)
                }
            }
            .controlSize(.small)
            Button("Add and start") {
                let urls = controller.urls(inPackageNamed: package.name)
                let name = package.name
                Task {
                    await engineController.addPackage(
                        name: name, urls: urls, startImmediately: true)
                }
            }
            .controlSize(.small)
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
                    .foregroundStyle(.secondary)
                Spacer()
                filterChip(.online, count: snapshot.onlineCount)
                filterChip(.faulty, count: snapshot.faultyCount)
                filterChip(.offline, count: snapshot.offlineCount)
                filterChip(.failed, count: snapshot.failedCount)
            }
        }
        .padding()
    }

    private func filterChip(_ filter: VerdictFilter, count: Int) -> some View {
        Button {
            activeFilter = activeFilter == filter ? .all : filter
        } label: {
            Text("\(filter.label) \(count)")
                .font(.caption)
        }
        .buttonStyle(.bordered)
        .tint(activeFilter == filter ? .accentColor : .secondary)
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

    var body: some View {
        HStack {
            Text(link.effectiveFilename).lineLimit(1)
            if link.isDuplicate {
                Text("duplicate")
                    .font(.caption)
                    .foregroundStyle(.orange)
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
            Text("online").font(.caption).foregroundStyle(.green)
        case .offline:
            Text("offline").font(.caption).foregroundStyle(.secondary)
        case .checkFailed:
            Text("check failed").font(.caption).foregroundStyle(.secondary)
        case .faulty(let reason):
            // Spec §7.3: the faulty reason *is* the badge text.
            Text(reason).font(.caption).foregroundStyle(.red)
        case nil:
            // No verdict yet: spec §7.5's queued → probing → sniffing → done
            // per-link state, shown literally rather than a bare spinner.
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text(stageLabel).font(.caption).foregroundStyle(.secondary)
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
