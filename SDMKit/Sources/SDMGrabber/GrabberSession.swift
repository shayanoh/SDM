import Foundation

/// Owns the batch of links being grabbed: extraction, connection-budgeted
/// probing, and package clustering. Spec §7.5.
public actor GrabberSession {
    public struct Budget: Sendable {
        public var globalMaxConcurrentProbes: Int
        public var maxConcurrentPerHost: Int

        public init(globalMaxConcurrentProbes: Int = 16, maxConcurrentPerHost: Int = 4) {
            precondition(globalMaxConcurrentProbes >= 1)
            precondition(maxConcurrentPerHost >= 1)
            self.globalMaxConcurrentProbes = globalMaxConcurrentProbes
            self.maxConcurrentPerHost = maxConcurrentPerHost
        }
    }

    private let prober: LinkProber
    private let budget: Budget
    private var links: [UUID: ProbedLink] = [:]
    private var order: [UUID] = []
    private var seenURLs: Set<URL> = []
    var knownDownloadURLs: Set<URL> = []
    private var packages: [PackageCandidate] = []
    private var manualOverrides: [UUID: String] = [:]

    public init(prober: LinkProber, budget: Budget = Budget()) {
        self.prober = prober
        self.budget = budget
    }

    /// Extracts links from pasted or dropped text, dedupes against links
    /// already in this session, then probes the new ones under budget.
    public func ingest(text: String) async {
        await ingest(urls: URLExtractor.extractLinks(from: text))
    }

    public func ingest(urls: [URL]) async {
        var fresh: [UUID] = []
        for url in urls where seenURLs.insert(url).inserted {
            let id = UUID()
            links[id] = ProbedLink(
                id: id, originalURL: url, isDuplicate: knownDownloadURLs.contains(url))
            order.append(id)
            fresh.append(id)
        }
        guard !fresh.isEmpty else { return }
        await probeBounded(fresh)
        recluster()
    }

    public func snapshot() -> GrabberSnapshot {
        let ordered = order.compactMap { links[$0] }
        return GrabberSnapshot(
            links: ordered,
            packages: packages,
            checkedCount: ordered.filter { $0.stage == .done }.count,
            totalCount: ordered.count
        )
    }

    /// Refreshes which grabbed links are already in the download list.
    /// Spec §7.5: badged as duplicates, never silently dropped.
    public func setKnownDownloadURLs(_ urls: Set<URL>) {
        knownDownloadURLs = urls
        for id in order {
            guard let url = links[id]?.originalURL else { continue }
            links[id]?.isDuplicate = urls.contains(url)
        }
    }

    public func removeLink(_ id: UUID) {
        guard let link = links.removeValue(forKey: id) else { return }
        seenURLs.remove(link.originalURL)
        order.removeAll { $0 == id }
        manualOverrides[id] = nil
        recluster()
    }

    /// Removes every member link of one package. Identified by id — see
    /// `moveLink(_:toPackage:)`'s doc comment for why.
    public func removePackage(_ id: UUID) {
        guard let package = packages.first(where: { $0.id == id }) else { return }
        for linkID in package.linkIDs {
            guard let link = links.removeValue(forKey: linkID) else { continue }
            seenURLs.remove(link.originalURL)
        }
        order.removeAll { linkID in package.linkIDs.contains(linkID) }
        for linkID in package.linkIDs { manualOverrides[linkID] = nil }
        recluster()
    }

    /// Removes every link in the session, clearing the list back to empty.
    public func clear() {
        links.removeAll()
        order.removeAll()
        seenURLs.removeAll()
        manualOverrides.removeAll()
        packages.removeAll()
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
        guard links[id] != nil, let package = packages.first(where: { $0.id == packageID }) else {
            return
        }
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
            guard let link = links[linkID] else { continue }
            manualOverrides[linkID] = link.effectiveFilename
        }
        recluster()
    }

    /// Re-runs the probe for every link whose most recent check failed
    /// outright (`.checkFailed`) — a transient network/transport error
    /// rather than a genuine verdict about the URL, so unlike faulty/offline
    /// it is worth retrying without the operator re-pasting the link.
    public func recheckFailed() async {
        let failedIDs = order.filter { links[$0]?.verdict == .checkFailed }
        guard !failedIDs.isEmpty else { return }
        await probeBounded(failedIDs)
        recluster()
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
    }

    private func recluster() {
        let clusterable = order.compactMap { id -> ClusterableLink? in
            guard let link = links[id] else { return nil }
            let host = link.finalURL.host ?? link.originalURL.host ?? ""
            let directoryPath = link.finalURL.deletingLastPathComponent().path
            return ClusterableLink(
                id: id, filename: link.effectiveFilename, host: host, directoryPath: directoryPath)
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

        packages = mergingDuplicateNames(candidates).map(reconcilingIdentity)
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
