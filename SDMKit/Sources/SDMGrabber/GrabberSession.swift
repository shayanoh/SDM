import Foundation
import SDMCore
import SDMResolve

/// Owns the batch of links being grabbed: extraction, connection-budgeted
/// probing, resolver-backed media resolution, and package clustering.
/// Spec §7.5 / §6.
public actor GrabberSession {
    public struct Budget: Sendable {
        public var globalMaxConcurrentProbes: Int
        public var maxConcurrentPerHost: Int
        /// How many yt-dlp `-J` extractions may run at once when expanding a
        /// playlist. Kept small and independent of the probe caps: each
        /// resolve spawns a Python process that runs a JS runtime and hits
        /// the network, so a big playlist at the probe concurrency (dozens)
        /// pins the CPU and every resolve times out. Parent spec §6.2.
        public var maxConcurrentResolves: Int

        public init(
            globalMaxConcurrentProbes: Int = 16,
            maxConcurrentPerHost: Int = 4,
            maxConcurrentResolves: Int = 4
        ) {
            precondition(globalMaxConcurrentProbes >= 1)
            precondition(maxConcurrentPerHost >= 1)
            precondition(maxConcurrentResolves >= 1)
            self.globalMaxConcurrentProbes = globalMaxConcurrentProbes
            self.maxConcurrentPerHost = maxConcurrentPerHost
            self.maxConcurrentResolves = maxConcurrentResolves
        }
    }

    private let prober: LinkProber
    private let budget: Budget
    private let resolver: (any LinkResolver)?
    private let qualityPreferences: @Sendable () -> QualityPreferences
    private let ffmpegAvailable: @Sendable () -> Bool
    private var links: [UUID: ProbedLink] = [:]
    private var mediaRows: [UUID: MediaRow] = [:]
    private var order: [UUID] = []
    private var seenURLs: Set<URL> = []
    var knownDownloadURLs: Set<URL> = []
    private var packages: [PackageCandidate] = []
    private var manualOverrides: [UUID: String] = [:]
    /// Note text keyed by package name, reattached after every `recluster()`
    /// — the mechanism for a truncated playlist's "50 of 320 videos".
    private var packageNotes: [String: String] = [:]
    /// When each row entered the session, for the "auto-clear after N
    /// minutes" setting.
    private var addedAt: [UUID: Date] = [:]
    /// Set by `cancelChecks()`; the probe and resolve loops stop launching
    /// new work while it's true. Cleared at the start of every ingest/recheck.
    private var cancelRequested = false

    public init(
        prober: LinkProber,
        budget: Budget = Budget(),
        resolver: (any LinkResolver)? = nil,
        qualityPreferences: @escaping @Sendable () -> QualityPreferences = { .default },
        ffmpegAvailable: @escaping @Sendable () -> Bool = { true }
    ) {
        self.prober = prober
        self.budget = budget
        self.resolver = resolver
        self.qualityPreferences = qualityPreferences
        self.ffmpegAvailable = ffmpegAvailable
    }

    /// Extracts links from pasted or dropped text, dedupes against links
    /// already in this session, then probes the new ones under budget.
    public func ingest(text: String) async {
        await ingest(urls: URLExtractor.extractLinks(from: text))
    }

    public func ingest(urls: [URL]) async {
        cancelRequested = false
        // Same gate as text extraction — a dropped bare-host URL is not a
        // download.
        let httpURLs = urls.filter(URLExtractor.isGrabbable)
        var freshProbes: [UUID] = []
        var freshMedia: [UUID] = []
        let now = Date()
        for url in httpURLs where seenURLs.insert(url).inserted {
            let id = UUID()
            addedAt[id] = now
            if resolver?.canHandle(url) == true {
                mediaRows[id] = MediaRow(
                    id: id, sourceURL: url, isDuplicate: knownDownloadURLs.contains(url))
                order.append(id)
                freshMedia.append(id)
            } else {
                links[id] = ProbedLink(
                    id: id, originalURL: url, isDuplicate: knownDownloadURLs.contains(url))
                order.append(id)
                freshProbes.append(id)
            }
        }
        guard !freshProbes.isEmpty || !freshMedia.isEmpty else { return }
        if !freshProbes.isEmpty { await probeBounded(freshProbes) }
        for id in freshMedia { await resolveMedia(id) }
        recluster()
    }

    // MARK: - Media resolution

    /// Resolves one media row through the injected resolver and runs
    /// auto-pick. A playlist URL fans out into one row per entry. Parent
    /// spec §6.2.
    private func resolveMedia(_ id: UUID) async {
        guard let resolver, let url = mediaRows[id]?.sourceURL else { return }
        let target: ResolvedTarget
        do {
            target = try await resolver.resolve(url)
        } catch {
            mediaRows[id]?.state = Self.rowState(for: error)
            return
        }
        switch target {
        case .single(let media):
            applyResolvedMedia(media, to: id)
        case .playlist(let title, let entries, let totalAvailable):
            await expandPlaylist(
                originID: id, originURL: url, title: title, entries: entries,
                totalAvailable: totalAvailable)
        }
    }

    private func applyResolvedMedia(_ media: ResolvedMedia, to id: UUID) {
        guard mediaRows[id] != nil else { return }
        mediaRows[id]?.media = media
        mediaRows[id]?.title = media.title
        guard let choice = FormatSelector.pick(media, qualityPreferences()) else {
            mediaRows[id]?.choice = nil
            mediaRows[id]?.state = .unselected
            return
        }
        mediaRows[id]?.choice = choice
        mediaRows[id]?.state = (choice.requiresMux && !ffmpegAvailable()) ? .needsFfmpeg : .resolved
    }

    private static func rowState(for error: any Error) -> MediaRowState {
        switch error {
        case ResolveError.binaryMissing: return .needsYtDlp
        case ResolveError.unsupported: return .unsupported
        case ResolveError.authRequired:
            return .failed(
                "YouTube blocked the request (anti-bot / sign-in). Set \"Cookies from browser\" "
                    + "in Settings → YouTube, or update yt-dlp.")
        case ResolveError.privateVideo: return .failed("Private video.")
        case ResolveError.unavailable: return .failed("Video unavailable.")
        case ResolveError.timeout: return .failed("yt-dlp timed out.")
        case ResolveError.formatGone: return .failed("The requested format is no longer available.")
        case ResolveError.ytDlpFailed(let stderrTail):
            // Surface the real yt-dlp error verbatim — the UI puts the full
            // text in the row's tooltip. A couple of known-cryptic YouTube
            // messages get a short hint prepended.
            let cleaned = stderrTail.trimmingCharacters(in: .whitespacesAndNewlines)
            if cleaned.isEmpty { return .failed("yt-dlp could not resolve this video.") }
            if cleaned.lowercased().contains("the page needs to be reloaded") {
                return .failed(
                    "YouTube rejected the session — your browser cookies are likely stale or "
                        + "locked. Quit the browser and retry, try a different browser, or update "
                        + "yt-dlp. (macOS: Chrome needs App-Bound decryption; Safari needs Full "
                        + "Disk Access for SDM.)\n\n" + cleaned)
            }
            return .failed(cleaned)
        default: return .failed("Could not resolve: \(error)")
        }
    }

    /// Sets a format on a media row (the picker override) and re-clusters —
    /// the derived filename changes. Parent spec §9.2.
    public func setFormatChoice(_ choice: FormatChoice, for id: UUID) {
        guard mediaRows[id] != nil else { return }
        mediaRows[id]?.choice = choice
        mediaRows[id]?.state = (choice.requiresMux && !ffmpegAvailable()) ? .needsFfmpeg : .resolved
        recluster()
    }

    /// Fans a playlist out into one row per entry, clusters them into one
    /// named package, and lazily resolves each entry's formats. Parent
    /// spec §6.2. Implemented in Part 4 Task 3.
    private func expandPlaylist(
        originID: UUID, originURL: URL, title: String, entries: [ResolvedMedia],
        totalAvailable: Int
    ) async {
        // Replace the origin row with one row per entry, all in one group.
        // The playlist URL itself no longer has a row, so it must leave
        // `seenURLs` — otherwise re-adding the same playlist later is
        // silently deduped and nothing happens.
        let group = UUID()
        order.removeAll { $0 == originID }
        mediaRows[originID] = nil
        seenURLs.remove(originURL)
        // The entries inherit the origin row's age, so a playlist added and
        // forgotten clears on the same schedule as anything else.
        let addedTime = addedAt[originID] ?? Date()
        addedAt[originID] = nil
        var entryIDs: [UUID] = []
        for entry in entries {
            let entryID = UUID()
            let watchURL = URL(string: "https://www.youtube.com/watch?v=\(entry.videoID)")!
            guard seenURLs.insert(watchURL).inserted else { continue }
            addedAt[entryID] = addedTime
            mediaRows[entryID] = MediaRow(
                id: entryID, sourceURL: watchURL, title: entry.title, state: .resolving,
                playlistGroup: group, isDuplicate: knownDownloadURLs.contains(watchURL))
            order.append(entryID)
            entryIDs.append(entryID)
        }
        for entryID in entryIDs { manualOverrides[entryID] = title }
        packageNotes[title] =
            entries.count < totalAvailable
            ? "\(entries.count) of \(totalAvailable) videos" : nil

        // Lazily resolve each entry, but only a few yt-dlp processes at once
        // — the probe concurrency (dozens) would pin the CPU and time every
        // extraction out. See `Budget.maxConcurrentResolves`.
        await withTaskGroup(of: (UUID, Result<ResolvedTarget, any Error>).self) { taskGroup in
            var pending = entryIDs
            var active = 0
            func launch() {
                while active < budget.maxConcurrentResolves, !pending.isEmpty {
                    let entryID = pending.removeFirst()
                    guard let url = mediaRows[entryID]?.sourceURL, let resolver else { continue }
                    active += 1
                    taskGroup.addTask {
                        do { return (entryID, .success(try await resolver.resolve(url))) } catch {
                            return (entryID, .failure(error))
                        }
                    }
                }
            }
            launch()
            while let (entryID, result) = await taskGroup.next() {
                active -= 1
                switch result {
                case .success(.single(let media)): applyResolvedMedia(media, to: entryID)
                case .success(.playlist): mediaRows[entryID]?.state = .unsupported
                case .failure(let error): mediaRows[entryID]?.state = Self.rowState(for: error)
                }
                launch()
            }
        }
    }

    public func snapshot() -> GrabberSnapshot {
        let orderedLinks = order.compactMap { links[$0] }
        let orderedMedia = order.compactMap { mediaRows[$0] }
        let mediaChecked = orderedMedia.filter {
            switch $0.state {
            case .resolving: return false
            default: return true
            }
        }.count
        return GrabberSnapshot(
            links: orderedLinks,
            mediaRows: orderedMedia,
            packages: packages,
            checkedCount: orderedLinks.filter { $0.stage == .done }.count + mediaChecked,
            totalCount: orderedLinks.count + orderedMedia.count
        )
    }

    /// Refreshes which grabbed links are already in the download list.
    /// Spec §7.5: badged as duplicates, never silently dropped.
    public func setKnownDownloadURLs(_ urls: Set<URL>) {
        knownDownloadURLs = urls
        for id in order {
            if let url = links[id]?.originalURL {
                links[id]?.isDuplicate = urls.contains(url)
            } else if let url = mediaRows[id]?.sourceURL {
                mediaRows[id]?.isDuplicate = urls.contains(url)
            }
        }
    }

    public func removeLink(_ id: UUID) {
        if let link = links.removeValue(forKey: id) {
            seenURLs.remove(link.originalURL)
        } else if let row = mediaRows.removeValue(forKey: id) {
            seenURLs.remove(row.sourceURL)
        } else {
            return
        }
        order.removeAll { $0 == id }
        manualOverrides[id] = nil
        addedAt[id] = nil
        recluster()
    }

    /// Removes every member link of one package. Identified by id — see
    /// `moveLink(_:toPackage:)`'s doc comment for why.
    public func removePackage(_ id: UUID) {
        guard let package = packages.first(where: { $0.id == id }) else { return }
        for linkID in package.linkIDs {
            if let link = links.removeValue(forKey: linkID) {
                seenURLs.remove(link.originalURL)
            } else if let row = mediaRows.removeValue(forKey: linkID) {
                seenURLs.remove(row.sourceURL)
            }
        }
        order.removeAll { linkID in package.linkIDs.contains(linkID) }
        for linkID in package.linkIDs {
            manualOverrides[linkID] = nil
            addedAt[linkID] = nil
        }
        packageNotes[package.name] = nil
        recluster()
    }

    /// Removes every link in the session, clearing the list back to empty.
    public func clear() {
        links.removeAll()
        mediaRows.removeAll()
        order.removeAll()
        seenURLs.removeAll()
        manualOverrides.removeAll()
        packageNotes.removeAll()
        addedAt.removeAll()
        packages.removeAll()
    }

    /// Removes every row that entered the session more than `maxAge` seconds
    /// ago — the "auto-clear grabbed links after N minutes" setting. `now`
    /// is injectable for tests.
    public func pruneOlderThan(_ maxAge: TimeInterval, now: Date = Date()) {
        let cutoff = now.addingTimeInterval(-maxAge)
        let expired = order.filter { (addedAt[$0] ?? now) < cutoff }
        guard !expired.isEmpty else { return }
        let expiredSet = Set(expired)
        for id in expired {
            if let link = links.removeValue(forKey: id) {
                seenURLs.remove(link.originalURL)
            } else if let row = mediaRows.removeValue(forKey: id) {
                seenURLs.remove(row.sourceURL)
            }
            manualOverrides[id] = nil
            addedAt[id] = nil
        }
        order.removeAll { expiredSet.contains($0) }
        recluster()
    }

    /// Forces a link into a package, overriding automatic clustering. Spec
    /// §7.4: "All of it is fully overridable."
    ///
    /// Identified by id, not name: `packages` can (however briefly) hold two
    /// entries with the same name — see `recluster()`'s duplicate-name
    /// merge — and even when it can't, a name captured by the caller earlier
    /// is not guaranteed to still uniquely pick out the package the operator
    /// actually meant.
    public func moveLink(_ id: UUID, toPackage packageID: UUID) {
        guard links[id] != nil || mediaRows[id] != nil,
            let package = packages.first(where: { $0.id == packageID })
        else { return }
        manualOverrides[id] = package.name
        recluster()
    }

    /// Renames a package by moving every member link's manual override to
    /// the new name. Spec §7.4: "rename." Identified by id — see
    /// `moveLink(_:toPackage:)`'s doc comment for why.
    public func renamePackage(_ id: UUID, to newName: String) {
        guard !newName.isEmpty, let package = packages.first(where: { $0.id == id }),
            package.name != newName
        else { return }
        for linkID in package.linkIDs { manualOverrides[linkID] = newName }
        if let note = packageNotes[package.name] {
            packageNotes[package.name] = nil
            packageNotes[newName] = note
        }
        recluster()
    }

    /// Combines one package's links into another's. Spec §7.4: "merge."
    /// Identified by id — see `moveLink(_:toPackage:)`'s doc comment for why.
    public func mergePackages(_ sourceID: UUID, into destinationID: UUID) {
        guard sourceID != destinationID,
            let source = packages.first(where: { $0.id == sourceID }),
            let destination = packages.first(where: { $0.id == destinationID })
        else { return }
        for linkID in source.linkIDs { manualOverrides[linkID] = destination.name }
        recluster()
    }

    /// Splits a package so each member link becomes its own single-link
    /// package, named after that link. Spec §7.4: "split." Identified by id
    /// — see `moveLink(_:toPackage:)`'s doc comment for why.
    public func splitPackage(_ id: UUID) {
        guard let package = packages.first(where: { $0.id == id }) else { return }
        for linkID in package.linkIDs {
            if let link = links[linkID] {
                manualOverrides[linkID] = link.effectiveFilename
            } else if let row = mediaRows[linkID] {
                manualOverrides[linkID] = row.displayFilename
            }
        }
        recluster()
    }

    /// Re-checks everything worth retrying: HTTP links whose probe failed
    /// outright (`.checkFailed`), and media rows that failed to resolve or
    /// are waiting on a now-maybe-installed binary. Faulty/offline links and
    /// `.unsupported` media rows are genuine verdicts and are left alone.
    public func recheckFailed() async {
        cancelRequested = false
        let failedProbes = order.filter { links[$0]?.verdict == .checkFailed }
        let failedMedia = order.filter { id in
            guard let state = mediaRows[id]?.state else { return false }
            switch state {
            case .failed, .needsYtDlp, .needsFfmpeg: return true
            default: return false
            }
        }
        guard !failedProbes.isEmpty || !failedMedia.isEmpty else { return }

        if !failedProbes.isEmpty { await probeBounded(failedProbes) }
        await resolveMediaBounded(failedMedia)
        recluster()
    }

    /// Re-probes / re-resolves exactly the given rows, whatever their current
    /// verdict — the per-item "Recheck" context-menu action. Unlike
    /// `recheckFailed()`, an already-online link or resolved media row is
    /// retried too, because the operator asked for it explicitly.
    public func recheck(ids: Set<UUID>) async {
        cancelRequested = false
        let probes = order.filter { ids.contains($0) && links[$0] != nil }
        let media = order.filter { ids.contains($0) && mediaRows[$0] != nil }
        guard !probes.isEmpty || !media.isEmpty else { return }

        if !probes.isEmpty { await probeBounded(probes) }
        await resolveMediaBounded(media)
        recluster()
    }

    /// Stops the in-flight probe and resolve loops from launching any further
    /// work. Rows already handed to a probe or a yt-dlp process finish and
    /// are recorded; rows still waiting are settled as check failures so they
    /// stay recheckable. The header's "Cancel Checks" button.
    public func cancelChecks() {
        cancelRequested = true
    }

    /// Re-resolves the given media rows under the small resolve concurrency
    /// cap — a burst of yt-dlp processes trips YouTube's rate limit.
    private func resolveMediaBounded(_ ids: [UUID]) async {
        var pending = ids.filter { mediaRows[$0] != nil }
        guard !pending.isEmpty else { return }
        for id in pending { mediaRows[id]?.state = .resolving }
        await withTaskGroup(of: Void.self) { group in
            var active = 0
            func launch() {
                if cancelRequested {
                    for id in pending { mediaRows[id]?.state = .failed("Check cancelled") }
                    pending.removeAll()
                    return
                }
                while active < budget.maxConcurrentResolves, !pending.isEmpty {
                    let id = pending.removeFirst()
                    guard mediaRows[id] != nil else { continue }
                    active += 1
                    group.addTask { [weak self] in await self?.resolveMedia(id) }
                }
            }
            launch()
            while await group.next() != nil {
                active -= 1
                launch()
            }
        }
    }

    // MARK: - Probing

    /// Launches probes for `ids` under the global and per-host caps, one
    /// wave of `TaskGroup` children at a time. All mutable state here
    /// (`links`, `hostCounts`, `pending`) is only touched synchronously
    /// between suspension points, so a concurrent `ingest` call started
    /// while this one is still probing sees a consistent, if interleaved,
    /// picture — the same actor-reentrancy shape `DownloadEngine.reconcile`
    /// documents in Phase 1.
    private func probeBounded(_ ids: [UUID]) async {
        // Resolved once, in actor-isolated code, so the `TaskGroup` closure
        // below never needs to read the actor-isolated `links` dictionary
        // from inside a nested predicate closure — only plain tuples.
        var pending: [(id: UUID, url: URL, host: String)] = ids.compactMap { id in
            guard let url = links[id]?.originalURL else { return nil }
            return (id, url, url.host ?? "")
        }
        var hostCounts: [String: Int] = [:]

        await withTaskGroup(of: (UUID, ProbedLink).self) { group in
            var active = 0

            func launchNext() {
                guard !cancelRequested else { return }
                while active < budget.globalMaxConcurrentProbes {
                    guard
                        let index = pending.firstIndex(where: { entry in
                            (hostCounts[entry.host] ?? 0) < budget.maxConcurrentPerHost
                        })
                    else { return }
                    let entry = pending.remove(at: index)
                    hostCounts[entry.host, default: 0] += 1
                    active += 1
                    links[entry.id]?.stage = .probing
                    group.addTask { [prober] in
                        (entry.id, await prober.probe(entry.url, id: entry.id))
                    }
                }
            }

            launchNext()
            while let (id, probed) = await group.next() {
                active -= 1
                if let host = links[id]?.originalURL.host {
                    hostCounts[host, default: 1] -= 1
                }
                var finished = probed
                finished.isDuplicate = knownDownloadURLs.contains(finished.originalURL)
                finished.verdict = VerdictRules.evaluate(finished)
                links[id] = finished
                launchNext()
            }
        }

        // Anything `cancelChecks()` kept from ever launching is left with no
        // verdict and a spinning row — settle it as a check failure so
        // "Recheck All" / the per-item Recheck picks it back up.
        if cancelRequested {
            for id in ids where links[id]?.verdict == nil {
                links[id]?.stage = .done
                links[id]?.verdict = .checkFailed
            }
        }
    }

    private func recluster() {
        let clusterable = order.compactMap { id -> ClusterableLink? in
            if let link = links[id] {
                let host = link.finalURL.host ?? link.originalURL.host ?? ""
                let directoryPath = link.finalURL.deletingLastPathComponent().path
                return ClusterableLink(
                    id: id, filename: link.effectiveFilename, host: host,
                    directoryPath: directoryPath)
            }
            if let row = mediaRows[id] {
                return ClusterableLink(
                    id: id, filename: row.displayFilename,
                    host: row.sourceURL.host ?? "youtube.com", directoryPath: "/watch")
            }
            return nil
        }
        var candidates = PackageClustering.cluster(clusterable)

        if !manualOverrides.isEmpty {
            for (id, name) in manualOverrides {
                for index in candidates.indices { candidates[index].linkIDs.removeAll { $0 == id } }
                if let index = candidates.firstIndex(where: { $0.name == name }) {
                    candidates[index].linkIDs.append(id)
                } else {
                    candidates.append(PackageCandidate(name: name, linkIDs: [id]))
                }
            }
            candidates = candidates.filter { !$0.linkIDs.isEmpty }
        }

        packages = mergingDuplicateNames(candidates).map(reconcilingIdentity).map {
            var candidate = $0
            candidate.note = packageNotes[candidate.name] ?? nil
            return candidate
        }
    }

    /// Clustering and manual overrides can each independently land two
    /// candidates on the same name — two ingested batches whose derived
    /// titles happen to coincide, or a rename/merge target that already
    /// matches another package. A name is the only thing that identifies a
    /// package to the operator, so two sharing one reads as a single,
    /// confusingly duplicated package rather than two — collapse them into
    /// one, keeping the earlier one's place. This is also the mechanism
    /// `renamePackage`/`mergePackages` themselves rely on: both work by
    /// giving the moved links' override the target's name and letting this
    /// merge fold them back together.
    private func mergingDuplicateNames(_ candidates: [PackageCandidate]) -> [PackageCandidate] {
        var merged: [PackageCandidate] = []
        for candidate in candidates {
            if let index = merged.firstIndex(where: { $0.name == candidate.name }) {
                merged[index].linkIDs.append(contentsOf: candidate.linkIDs)
                merged[index].isArchive = merged[index].isArchive || candidate.isArchive
            } else {
                merged.append(candidate)
            }
        }
        return merged
    }

    /// Gives a candidate the `id` of whichever *previous* package (from
    /// before this `recluster()` call) already had the same name, so a
    /// package's identity survives reclustering instead of becoming a new
    /// id — and hence a new package as far as any id-keyed UI state is
    /// concerned — every time a link is added, removed, or just reordered
    /// underneath an unchanged name. A name with no previous match keeps the
    /// fresh id `PackageClustering`/`PackageCandidate.init` already gave it.
    private func reconcilingIdentity(_ candidate: PackageCandidate) -> PackageCandidate {
        var candidate = candidate
        if let previous = packages.first(where: { $0.name == candidate.name }) {
            candidate.id = previous.id
        }
        return candidate
    }
}
