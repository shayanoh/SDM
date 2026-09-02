import SDMCore
import SDMGrabber
import SwiftUI
import UniformTypeIdentifiers

#Preview {
    LinkRow(
        link:
            ProbedLink(
                id: UUID(), originalURL: URL(fileURLWithPath: ""), finalURL: nil, stage: .done,
                statusCode: 200, contentLength: 1_000_000_000, contentType: nil,
                suggestedFilename: "Filename", validator: "validator", acceptsRanges: true,
                sniffedSignature: nil, transportFailed: false, verdict: .online, isDuplicate: false),
        controller: GrabberController(),
        theme: ThemeCatalog.builtInThemes()[5])
}
struct LinkGrabberView: View {
    @Environment(GrabberController.self) private var controller
    @Environment(EngineController.self) private var engineController
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
    @State private var isShowingAddSheet = false
    @State private var isShowingRenameAlert = false
    @State private var renamingPackageID: UUID?
    @State private var newPackageName = ""
    @State private var heldBackMessage: String?

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    var body: some View {
        List {
            ForEach(linksInNoPackage()) {
                LinkRow(link: $0, controller: controller, theme: theme)
            }
            ForEach(mediaRowsInNoPackage()) {
                MediaLinkRow(row: $0, controller: controller, theme: theme)
            }
            ForEach(controller.snapshot.packages) { package in
                // Deliberately not a `Section` with a `header:` — macOS
                // renders a `Section` header as an AppKit "group row"
                // (`NSTableView`'s floating-header chrome), which paints
                // its own background regardless of `.listRowBackground`
                // or `.listStyle`. A package header here is instead a
                // plain sibling row ahead of its links, the same way
                // `PackagesListView` avoids the same trap by using
                // `DisclosureGroup` rather than `Section` — a normal row
                // that `.listRowBackground` actually controls.
                packageHeader(package)
                    .listRowBackground(theme.surfaceSecondaryColor)
                ForEach(links(in: package)) { link in
                    LinkRow(link: link, controller: controller, theme: theme)
                        .draggable(DraggedLinkID(linkID: link.id))
                }
                ForEach(mediaRows(in: package)) { row in
                    MediaLinkRow(row: row, controller: controller, theme: theme)
                        .draggable(DraggedLinkID(linkID: row.id))
                }
            }
        }
        // `List` paints its own opaque system background regardless of what
        // sits behind it — without hiding that, `surfacePrimary` never shows.
        .scrollContentBackground(.hidden)
        .background(theme.surfacePrimaryColor)
        .listStyle(.plain)
        // The header is a `safeAreaInset` rather than a sibling in a `VStack`
        // so the `List` reserves space for it and its scroll content starts
        // *below* it — a `VStack { header; List }` let a freshly-added first
        // row render underneath the header until you scrolled.
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(spacing: 0) {
                header
                Divider()
            }
        }
        .alert("Rename package", isPresented: $isShowingRenameAlert) {
            TextField("Name", text: $newPackageName)
            Button("Rename") {
                guard let id = renamingPackageID else { return }
                let new = newPackageName
                Task { await controller.renamePackage(id, to: new) }
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
                Button("Add Links", systemImage: "link.badge.plus") { isShowingAddSheet = true }
            }
        }
        .sheet(isPresented: $isShowingAddSheet) {
            AddLinksSheet()
        }
        .frame(minWidth: 640, minHeight: 420)
        .task {
            while !Task.isCancelled {
                await updateControllerSnapshot()

                try? await Task.sleep(for: .seconds(0.25))
            }
        }
    }

    @ViewBuilder
    private func packageHeader(_ package: PackageCandidate) -> some View {
        HStack {
            Text(package.name)
                .font(.title3.bold())
            Spacer()
            Button("Add to downloads") {
                addToDownloads(package, startImmediately: false)
            }
            .controlSize(.small)
            Button("Add and start") {
                addToDownloads(package, startImmediately: true)
            }
            .controlSize(.small)
            Button(role: .destructive) {
                let id = package.id
                Task { await controller.removePackage(id) }
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.plain)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        // Without this, the HStack's own hit area is only as big as its
        // rendered content (the text, the buttons) — right-clicking the
        // `Spacer()`'s empty space did nothing. `contentShape` extends the
        // container's hit area to its full bounds for the context menu
        // without stealing taps from the buttons above: SwiftUI still
        // routes a click to the deepest view under it first, so each
        // button keeps getting its own clicks.
        .contentShape(Rectangle())
        .dropDestination(for: DraggedLinkID.self) { dragged, _ in
            guard let dragged = dragged.first else { return false }
            let packageID = package.id
            Task { await controller.moveLink(dragged.linkID, toPackage: packageID) }
            return true
        }
        .contextMenu {
            Button("Rename…") {
                renamingPackageID = package.id
                newPackageName = package.name
                isShowingRenameAlert = true
            }
            let others = controller.snapshot.packages.filter { $0.id != package.id }
            Menu("Merge into") {
                if others.isEmpty {
                    Button("No Other Packages") {}
                        .disabled(true)
                } else {
                    ForEach(others) { other in
                        Button(other.name) {
                            let source = package.id
                            let destination = other.id
                            Task { await controller.mergePackages(source, into: destination) }
                        }
                    }
                }
            }
            Button("Split") {
                let id = package.id
                Task { await controller.splitPackage(id) }
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
                statPill(label: "Online", count: snapshot.onlineCount, color: theme.onlineColor)
                statPill(label: "Faulty", count: snapshot.faultyCount, color: theme.faultyColor)
                statPill(label: "Offline", count: snapshot.offlineCount, color: theme.offlineColor)
                statPill(
                    label: "Check failed", count: snapshot.failedCount, color: theme.failedColor)
                Button("Recheck All") {
                    Task { await controller.recheckFailed() }
                }
                .controlSize(.small)
                .disabled(snapshot.recheckableCount == 0)
                Button("Clear", role: .destructive) {
                    Task { await controller.clear() }
                }
                .controlSize(.small)
                .disabled(snapshot.packages.isEmpty)
            }
            if let heldBackMessage {
                Text(heldBackMessage)
                    .font(.caption)
                    .foregroundStyle(theme.faultyColor)
            }
        }
        .padding()
        // Applied before `sdmSurface` so the opaque theme color is what's
        // actually visible — `sdmSurface`'s translucent material alone has
        // no theme color of its own, it just blurs whatever sits behind it.
        .background(theme.surfaceSecondaryColor)
        .sdmSurface(.toolbar)
    }

    /// A static readout, not a control — spec change: these used to be
    /// filter toggles, but a link grabber batch is small enough that
    /// hiding rows behind a filter was never worth the extra click. The
    /// full list always shows; these just report counts.
    private func statPill(label: String, count: Int, color: Color) -> some View {
        Text("\(label) \(count)")
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Capsule().fill(theme.surfaceTertiaryColor))
            .foregroundStyle(color)
    }

    private func updateControllerSnapshot() async {
        await controller.updateSnapshot()
    }

    /// Hands the package's links to the engine, then clears them out of the
    /// grabber — once a link is a download it has no reason to still show up
    /// as something waiting to be grabbed.
    private func addToDownloads(_ package: PackageCandidate, startImmediately: Bool) {
        let (items, heldBack) = controller.downloadItems(inPackage: package.id)
        guard !items.isEmpty || heldBack > 0 else { return }
        heldBackMessage =
            heldBack > 0
            ? "\(heldBack) item\(heldBack == 1 ? "" : "s") held back — choose a format first"
            : nil
        guard !items.isEmpty else { return }
        let name = package.name
        let note = package.note
        // Only the rows that actually handed off leave the grabber; held-back
        // media rows (no format yet) stay put.
        let linkIDs = controller.handoffLinkIDs(inPackage: package.id)
        Task {
            await engineController.addItems(
                name: name, note: note, items: items, startImmediately: startImmediately)
            for linkID in linkIDs {
                await controller.removeLink(linkID)
            }
        }
    }

    private func links(in package: PackageCandidate) -> [ProbedLink] {
        let ids = Set(package.linkIDs)
        return controller.snapshot.links.filter { ids.contains($0.id) }
    }

    private func linksInNoPackage() -> [ProbedLink] {
        let packagedLinkIDs = Set(controller.snapshot.packages.flatMap(\.linkIDs))
        return controller.snapshot.links.filter { !packagedLinkIDs.contains($0.id) }
    }

    private func mediaRows(in package: PackageCandidate) -> [MediaRow] {
        let ids = Set(package.linkIDs)
        return controller.snapshot.mediaRows.filter { ids.contains($0.id) }
    }

    private func mediaRowsInNoPackage() -> [MediaRow] {
        let packaged = Set(controller.snapshot.packages.flatMap(\.linkIDs))
        return controller.snapshot.mediaRows.filter { !packaged.contains($0.id) }
    }
}

private struct MediaLinkRow: View {
    let row: MediaRow
    let controller: GrabberController
    let theme: Theme

    var body: some View {
        HStack {
            Image(systemName: "play.rectangle")
                .foregroundStyle(theme.textSecondaryColor)
            Text(row.title).lineLimit(1)
                .help(row.sourceURL.absoluteString)
            if row.isDuplicate {
                Text("duplicate").font(.caption).foregroundStyle(theme.faultyColor)
            }
            Spacer()
            if let media = row.media, canPick {
                formatPicker(media)
            } else {
                badge
            }
            Text(sizeText).font(.caption).foregroundStyle(theme.textSecondaryColor)
        }
        .padding(.vertical, 2)
    }

    private var canPick: Bool {
        switch row.state {
        case .resolved, .unselected, .needsFfmpeg: return true
        default: return false
        }
    }

    @ViewBuilder
    private func formatPicker(_ media: SDMCore.ResolvedMedia) -> some View {
        let options = MediaFormatMenu.options(
            for: media, preferences: YouTubeSettingsStore.qualityPreferences)
        let matching = options.filter(\.matchesPreferences)
        let nonMatching = options.filter { !$0.matchesPreferences }
        Menu {
            ForEach(matching) { option in
                Button(option.label) { pick(option.choice) }
            }
            if !matching.isEmpty && !nonMatching.isEmpty { Divider() }
            ForEach(nonMatching) { option in
                Button("⚠ " + option.label) { pick(option.choice) }
            }
        } label: {
            Text(pickerLabel)
                .font(.caption)
                .foregroundStyle(row.state == .resolved ? theme.onlineColor : theme.faultyColor)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
    }

    private func pick(_ choice: SDMCore.FormatChoice) {
        let id = row.id
        Task { await controller.setFormatChoice(choice, for: id) }
    }

    private var pickerLabel: String {
        switch row.state {
        case .resolved: return formatSummary
        case .needsFfmpeg: return "requires ffmpeg"
        default: return "choose a format"
        }
    }

    @ViewBuilder
    private var badge: some View {
        switch row.state {
        case .resolving:
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text("resolving").font(.caption).foregroundStyle(theme.textSecondaryColor)
            }
        case .resolved:
            Text(formatSummary).font(.caption).foregroundStyle(theme.onlineColor)
        case .unselected:
            Text("choose a format").font(.caption).foregroundStyle(theme.faultyColor)
        case .unsupported:
            Text("not supported yet").font(.caption).foregroundStyle(theme.faultyColor)
        case .needsYtDlp:
            Text("requires yt-dlp").font(.caption).foregroundStyle(theme.faultyColor)
        case .needsFfmpeg:
            Text("requires ffmpeg").font(.caption).foregroundStyle(theme.faultyColor)
        case .failed(let reason):
            // The reason can be a multi-line yt-dlp error; show the first
            // line inline and the whole thing on hover.
            Text(reason.split(separator: "\n").first.map(String.init) ?? reason)
                .font(.caption)
                .foregroundStyle(theme.offlineColor)
                .lineLimit(1)
                .truncationMode(.tail)
                .textSelection(.enabled)
                .help(reason)
        }
    }

    private var formatSummary: String {
        guard let choice = row.choice else { return "resolved" }
        var parts: [String] = []
        if let h = choice.video?.height { parts.append("\(h)p") }
        if let v = choice.video?.vcodec { parts.append("\(v)") }
        parts.append(choice.outputContainer.fileExtension)
        return parts.joined(separator: " · ")
    }

    private var sizeText: String {
        guard let bytes = row.combinedBytes, bytes > 0 else { return "" }
        let f = ByteCountFormatter()
        f.countStyle = .binary
        return f.string(fromByteCount: bytes)
    }
}

private struct LinkRow: View {
    let link: ProbedLink
    let controller: GrabberController
    let theme: Theme

    var body: some View {
        VStack {
            HStack {
                Text(link.effectiveFilename).lineLimit(1)
                    .help(link.finalURL.absoluteString)
                if link.isDuplicate {
                    Text("duplicate")
                        .font(.caption)
                        .foregroundStyle(theme.faultyColor)
                }
                Spacer()
                verdictBadge
                Text(fileSize)
                if link.verdict != nil {
                    Button(role: .destructive) {
                        let id = link.id
                        Task { await controller.removeLink(id) }
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.plain)
                }
            }
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

    private var fileSize: String {
        guard let len = link.contentLength, len > 0 else { return "" }
        let formatter = ByteCountFormatter()
        formatter.countStyle = .binary
        return formatter.string(fromByteCount: len)
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
