import Foundation
import SDMCore

public struct PersistedState: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 1

    public var formatVersion: Int
    public var packages: [DownloadPackage]

    public init(
        formatVersion: Int = PersistedState.currentFormatVersion,
        packages: [DownloadPackage] = []
    ) {
        self.formatVersion = formatVersion
        self.packages = packages
    }

    /// Whether every package and item satisfies the invariants their own
    /// memberwise initializers enforce with `precondition` (non-empty
    /// `name` / `filename`).
    ///
    /// `Codable`'s synthesized `init(from:)` does not route through those
    /// initializers, so a corrupt or hand-edited snapshot can decode into a
    /// model that violates them without the precondition ever firing.
    /// Validating here, once, after decode, is how the store closes that
    /// gap instead of trusting the synthesized decoder.
    fileprivate var isValid: Bool {
        packages.allSatisfy { package in
            !package.name.isEmpty && package.items.allSatisfy { !$0.filename.isEmpty }
        }
    }
}

public protocol StateStore: Sendable {
    func load() async -> PersistedState
    func save(_ state: PersistedState) async
    /// Writes any pending state immediately. Call on quit and on a debounce timer.
    func flush() async
}

/// Durable state as a single atomically replaced JSON file.
///
/// Saves are coalesced in memory until `flush()`; the caller owns the debounce
/// timer, so no test has to wait on an internal one. See spec §4.2.
public actor JSONStateStore: StateStore {
    private let fileURL: URL
    private var pending: PersistedState?

    public init(fileURL: URL) {
        self.fileURL = fileURL
    }

    /// Loads the durable snapshot, returning an empty `PersistedState` when
    /// the file is missing, unreadable, garbage, written by a mismatched
    /// format version, or decodes into a package/item that violates the
    /// non-empty `name` / `filename` invariant those models otherwise
    /// enforce with `precondition` in their initializers.
    ///
    /// Deliberately non-throwing, matching `ResumeSidecar.load`: an unusable
    /// snapshot always means "start from empty", never "retry the load".
    public func load() -> PersistedState {
        if let pending { return pending }
        guard let data = try? Data(contentsOf: fileURL),
            let state = try? JSONDecoder().decode(PersistedState.self, from: data),
            state.formatVersion == PersistedState.currentFormatVersion,
            state.isValid
        else { return PersistedState() }
        return state
    }

    public func save(_ state: PersistedState) {
        pending = state
    }

    public func flush() {
        guard let state = pending else { return }
        pending = nil
        guard let data = try? JSONEncoder().encode(state) else { return }
        try? FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? data.write(to: fileURL, options: .atomic)
    }
}

/// Non-persistent store for tests and SwiftUI previews.
public actor InMemoryStateStore: StateStore {
    private var state = PersistedState()

    public init() {}

    public func load() -> PersistedState { state }
    public func save(_ state: PersistedState) { self.state = state }
    public func flush() {}
}
