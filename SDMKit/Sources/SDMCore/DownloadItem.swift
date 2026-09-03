import Foundation

public struct DownloadItem: Identifiable, Equatable, Sendable {
    public let id: UUID
    /// Invariant: at least one. A generic HTTP download has exactly one;
    /// a muxed YouTube download has two (video + audio). Parent spec §5.
    public var components: [FileComponent]
    /// The user-facing origin of the download: the URL that was grabbed.
    /// For a generic HTTP download this equals the (single) component's URL;
    /// for a YouTube item it is the `watch?v=` / `youtu.be` URL, while each
    /// component's own `url` is the direct `googlevideo` stream.
    public var sourceURL: URL
    /// The final file once assembly (if any) completes.
    public var outputFilename: String
    public var assembly: Assembly
    public var state: ItemState
    /// Purely user-managed: "never start this, no matter what." Independent
    /// of `state` — see `ItemState`. Only Disable/Enable changes it.
    public var isEnabled: Bool
    public var priority: Priority?
    /// Position within the owning package. Lower sorts earlier.
    public var position: Int

    public init(
        id: UUID = UUID(),
        components: [FileComponent],
        outputFilename: String,
        sourceURL: URL? = nil,
        assembly: Assembly = .none,
        state: ItemState = .queued,
        isEnabled: Bool = true,
        priority: Priority? = nil,
        position: Int = 0
    ) {
        precondition(!components.isEmpty, "a DownloadItem needs at least one component")
        precondition(!outputFilename.isEmpty, "outputFilename must not be empty")
        self.init(
            unchecked: id, components: components, outputFilename: outputFilename,
            sourceURL: sourceURL ?? components[0].url,
            assembly: assembly, state: state, isEnabled: isEnabled, priority: priority,
            position: position)
    }

    /// Non-validating path used only by `init(from:)`. Matches
    /// `DownloadPackage`/pre-Part-5 `DownloadItem`, whose synthesized
    /// `Codable` never routed through their preconditioned initializers —
    /// `PersistedState.isValid` is what rejects a corrupt snapshot after
    /// decode, and it can only do that if decoding a bad value does not trap.
    private init(
        unchecked id: UUID,
        components: [FileComponent],
        outputFilename: String,
        sourceURL: URL,
        assembly: Assembly,
        state: ItemState,
        isEnabled: Bool,
        priority: Priority?,
        position: Int
    ) {
        self.id = id
        self.components = components
        self.outputFilename = outputFilename
        self.sourceURL = sourceURL
        self.assembly = assembly
        self.state = state
        self.isEnabled = isEnabled
        self.priority = priority
        self.position = position
    }

    /// The pre-Part-5 signature. Wraps a single URL into a one-component
    /// HTTP item so every existing call site keeps compiling unchanged.
    public init(
        id: UUID = UUID(),
        url: URL,
        filename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        state: ItemState = .queued,
        isEnabled: Bool = true,
        isResumable: Bool? = nil,
        priority: Priority? = nil,
        position: Int = 0,
        validator: String? = nil
    ) {
        self.init(
            id: id,
            components: [
                FileComponent(
                    url: url, partFilename: filename, totalBytes: totalBytes,
                    completed: completed, validator: validator, origin: .http,
                    isResumable: isResumable)
            ],
            outputFilename: filename,
            assembly: .none,
            state: state,
            isEnabled: isEnabled,
            priority: priority,
            position: position)
    }

    // MARK: - Concatenated accessors (one-component: identical to before)

    /// The grabbed origin URL (`sourceURL`). For HTTP this is the file URL;
    /// for YouTube it is the watch URL, not the `googlevideo` stream.
    public var url: URL { sourceURL }
    public var filename: String { outputFilename }

    public var validator: String? {
        get { components[0].validator }
        set { for index in components.indices { components[index].validator = newValue } }
    }

    /// A wholesale (yt-dlp-as-downloader) component makes the whole item
    /// non-resumable and its scheduler slot reserved. Parent spec
    /// `2026-09-03-multi-site-resolver-design.md` §6.7.
    public var hasWholesaleComponent: Bool {
        components.contains {
            if case .wholesale = $0.origin { return true } else { return false }
        }
    }

    public var isResumable: Bool? {
        get {
            if hasWholesaleComponent { return false }
            if components.contains(where: { $0.isResumable == false }) { return false }
            if components.contains(where: { $0.isResumable == nil }) { return nil }
            return true
        }
        set { for index in components.indices { components[index].isResumable = newValue } }
    }

    public var totalBytes: Int64? {
        get {
            guard components.allSatisfy({ $0.totalBytes != nil }) else { return nil }
            return components.reduce(0) { $0 + ($1.totalBytes ?? 0) }
        }
        set {
            precondition(components.count == 1, "set totalBytes only on a one-component item")
            components[0].totalBytes = newValue
        }
    }

    /// Item-space completed ranges: each component's ranges shifted by the
    /// sum of the sizes before it. Falls back to component 0 alone while any
    /// size is still unknown.
    public var completed: RangeSet {
        get {
            let bases = componentBaseOffsets
            guard bases.count == components.count else { return components[0].completed }
            var union = RangeSet()
            for (index, component) in components.enumerated() {
                for range in component.completed.ranges {
                    union.insert(
                        ByteRange(start: range.start + bases[index], end: range.end + bases[index]))
                }
            }
            return union
        }
        set {
            precondition(components.count == 1, "set completed only on a one-component item")
            components[0].completed = newValue
        }
    }

    /// Running prefix sums of component sizes, or `[]` when any size is
    /// unknown (so callers fall back rather than misplace ranges).
    public var componentBaseOffsets: [Int64] {
        guard components.allSatisfy({ $0.totalBytes != nil }) else { return [] }
        var bases: [Int64] = []
        var running: Int64 = 0
        for component in components {
            bases.append(running)
            running += component.totalBytes ?? 0
        }
        return bases
    }

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        let done = components.reduce(Int64(0)) { $0 + $1.completed.totalBytes }
        return Double(done) / Double(total)
    }

    public var isComplete: Bool {
        components.allSatisfy(\.isComplete)
    }
}

// MARK: - Codable (accepts the legacy flat shape and the new nested one)

extension DownloadItem: Codable {
    private enum CodingKeys: String, CodingKey {
        case id, components, outputFilename, sourceURL, assembly, state, isEnabled, priority,
            position
        // legacy-only keys
        case url, filename, totalBytes, completed, isResumable, validator
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let id = try container.decode(UUID.self, forKey: .id)
        let state = try container.decodeIfPresent(ItemState.self, forKey: .state) ?? .queued
        let isEnabled = try container.decodeIfPresent(Bool.self, forKey: .isEnabled) ?? true
        let priority = try container.decodeIfPresent(Priority.self, forKey: .priority)
        let position = try container.decodeIfPresent(Int.self, forKey: .position) ?? 0

        let components: [FileComponent]
        let outputFilename: String
        let assembly: Assembly
        let sourceURL: URL?
        if container.contains(.components) {
            components = try container.decode([FileComponent].self, forKey: .components)
            outputFilename = try container.decode(String.self, forKey: .outputFilename)
            assembly = try container.decodeIfPresent(Assembly.self, forKey: .assembly) ?? .none
            // Absent in state.json written before `sourceURL` existed —
            // fall back to the first component's URL, the old behaviour.
            sourceURL = try container.decodeIfPresent(URL.self, forKey: .sourceURL)
        } else {
            let filename = try container.decode(String.self, forKey: .filename)
            let legacyURL = try container.decode(URL.self, forKey: .url)
            sourceURL = legacyURL
            components = [
                FileComponent(
                    url: legacyURL,
                    partFilename: filename,
                    totalBytes: try container.decodeIfPresent(Int64.self, forKey: .totalBytes),
                    completed: try container.decodeIfPresent(RangeSet.self, forKey: .completed)
                        ?? RangeSet(),
                    validator: try container.decodeIfPresent(String.self, forKey: .validator),
                    origin: .http,
                    isResumable: try container.decodeIfPresent(Bool.self, forKey: .isResumable))
            ]
            outputFilename = filename
            assembly = .none
        }
        self.init(
            unchecked: id, components: components, outputFilename: outputFilename,
            sourceURL: sourceURL ?? components[0].url,
            assembly: assembly, state: state, isEnabled: isEnabled, priority: priority,
            position: position)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(components, forKey: .components)
        try container.encode(outputFilename, forKey: .outputFilename)
        try container.encode(sourceURL, forKey: .sourceURL)
        try container.encode(assembly, forKey: .assembly)
        try container.encode(state, forKey: .state)
        try container.encode(isEnabled, forKey: .isEnabled)
        try container.encodeIfPresent(priority, forKey: .priority)
        try container.encode(position, forKey: .position)
    }
}
