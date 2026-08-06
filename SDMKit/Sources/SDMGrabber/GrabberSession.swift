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

    /// Forces a link into a named package, overriding automatic clustering.
    /// Spec §7.4: "All of it is fully overridable."
    public func moveLink(_ id: UUID, toPackageNamed name: String) {
        guard links[id] != nil else { return }
        manualOverrides[id] = name
        recluster()
    }

    /// Renames a package by moving every member link's manual override to
    /// the new name. Spec §7.4: "rename."
    public func renamePackage(_ oldName: String, to newName: String) {
        guard !newName.isEmpty, oldName != newName,
            let package = packages.first(where: { $0.name == oldName })
        else { return }
        for id in package.linkIDs { manualOverrides[id] = newName }
        recluster()
    }

    /// Combines one package's links into another's. Spec §7.4: "merge."
    public func mergePackages(_ sourceName: String, into destinationName: String) {
        guard sourceName != destinationName,
            let source = packages.first(where: { $0.name == sourceName })
        else { return }
        for id in source.linkIDs { manualOverrides[id] = destinationName }
        recluster()
    }

    /// Splits a package so each member link becomes its own single-link
    /// package, named after that link. Spec §7.4: "split."
    public func splitPackage(_ name: String) {
        guard let package = packages.first(where: { $0.name == name }) else { return }
        for id in package.linkIDs {
            guard let link = links[id] else { continue }
            manualOverrides[id] = link.effectiveFilename
        }
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

        guard !manualOverrides.isEmpty else {
            packages = candidates
            return
        }
        for (id, name) in manualOverrides {
            for index in candidates.indices { candidates[index].linkIDs.removeAll { $0 == id } }
            if let index = candidates.firstIndex(where: { $0.name == name }) {
                candidates[index].linkIDs.append(id)
            } else {
                candidates.append(PackageCandidate(name: name, linkIDs: [id]))
            }
        }
        packages = candidates.filter { !$0.linkIDs.isEmpty }
    }
}
