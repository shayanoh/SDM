import Foundation

public struct ClusterableLink: Identifiable, Equatable, Sendable {
    public let id: UUID
    public let filename: String
    public let host: String
    public let directoryPath: String

    public init(id: UUID, filename: String, host: String, directoryPath: String) {
        self.id = id
        self.filename = filename
        self.host = host
        self.directoryPath = directoryPath
    }
}

public struct PackageCandidate: Equatable, Sendable, Identifiable {
    /// Stable identity, independent of `name` — two packages can legitimately
    /// share a name for a moment (see `GrabberSession.recluster()`'s
    /// duplicate-name merge), and even when they don't, `name` alone is not
    /// enough for a caller to say "act on *this* package" unambiguously.
    /// `GrabberSession` preserves this id across reclustering for whichever
    /// previous package had the same name, so it stays stable across a mere
    /// regrouping rather than becoming a new id every tick.
    public var id: UUID
    public var name: String
    public var linkIDs: [UUID]
    public var isArchive: Bool

    public init(id: UUID = UUID(), name: String, linkIDs: [UUID], isArchive: Bool = false) {
        self.id = id
        self.name = name
        self.linkIDs = linkIDs
        self.isArchive = isArchive
    }
}

/// A pure function `[ClusterableLink] -> [PackageCandidate]`. Spec §7.4.
public enum PackageClustering {
    public static func cluster(_ links: [ClusterableLink]) -> [PackageCandidate] {
        guard !links.isEmpty else { return [] }
        let inputOrder = Dictionary(uniqueKeysWithValues: links.enumerated().map { ($1.id, $0) })

        var archiveGroups: [String: [ClusterableLink]] = [:]
        var remaining: [ClusterableLink] = []
        for candidate in links {
            if let base = archiveBaseName(candidate.filename) {
                archiveGroups[base, default: []].append(candidate)
            } else {
                remaining.append(candidate)
            }
        }

        var templateGroups: [String: [ClusterableLink]] = [:]
        for candidate in remaining {
            templateGroups[template(for: candidate.filename), default: []].append(candidate)
        }

        var candidates: [PackageCandidate] = []
        var singletons: [ClusterableLink] = []
        for members in templateGroups.values {
            if members.count > 1 {
                candidates.append(
                    PackageCandidate(name: name(for: members), linkIDs: members.map(\.id)))
            } else {
                singletons.append(contentsOf: members)
            }
        }

        var byHostPath: [String: [ClusterableLink]] = [:]
        for candidate in singletons {
            byHostPath["\(candidate.host)|\(candidate.directoryPath)", default: []].append(
                candidate)
        }
        for members in byHostPath.values {
            candidates.append(
                PackageCandidate(name: name(for: members), linkIDs: members.map(\.id)))
        }

        for (base, members) in archiveGroups {
            let cleaned = base.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
            candidates.append(
                PackageCandidate(
                    name: cleaned.isEmpty ? name(for: members) : beautify(cleaned),
                    linkIDs: members.map(\.id),
                    isArchive: true
                )
            )
        }

        // `Dictionary` iteration order is randomized per process; sort by
        // each candidate's earliest-appearing member so output is
        // deterministic across calls and test runs.
        return candidates.sorted { a, b in
            let aMin = a.linkIDs.compactMap { inputOrder[$0] }.min() ?? .max
            let bMin = b.linkIDs.compactMap { inputOrder[$0] }.min() ?? .max
            return aMin < bMin
        }
    }

    /// Lowercased, extension stripped, separators normalized, digit runs
    /// collapsed to `#` so `Show.S01E01.mkv` and `Show.S01E02.mkv` reduce to
    /// the same template and cluster with no episode-specific regex.
    private static func template(for filename: String) -> String {
        let stem = stripExtension(filename).lowercased()
        var result = ""
        var lastWasDigit = false
        for character in stem {
            if character.isLetter || character.isNumber {
                if character.isNumber {
                    if !lastWasDigit { result.append("#") }
                    lastWasDigit = true
                } else {
                    result.append(character)
                    lastWasDigit = false
                }
            } else {
                result.append(" ")
                lastWasDigit = false
            }
        }
        return result.split(separator: " ").joined(separator: " ")
    }

    /// Detects `.part01.rar`, `.r00`, `.z01`, and `.001`-style archive
    /// parts, returning the shared base name that locks them into one
    /// package regardless of template.
    private static func archiveBaseName(_ filename: String) -> String? {
        let lower = filename.lowercased()
        let patterns = [#"\.part\d+\.rar$"#, #"\.r\d\d$"#, #"\.z\d\d$"#, #"\.\d{3}$"#]
        for pattern in patterns {
            guard let regex = try? NSRegularExpression(pattern: pattern) else { continue }
            let range = NSRange(lower.startIndex..<lower.endIndex, in: lower)
            guard let match = regex.firstMatch(in: lower, range: range),
                let matchRange = Range(match.range, in: lower)
            else { continue }
            return String(lower[lower.startIndex..<matchRange.lowerBound])
        }
        return nil
    }

    private static func stripExtension(_ filename: String) -> String {
        URL(fileURLWithPath: filename).deletingPathExtension().lastPathComponent
    }

    /// Cleaned longest common prefix of member stems, falling back to the
    /// host. Tries the season/episode-aware prefix first, since a naive
    /// character-by-character common prefix mangles that specific pattern.
    private static func name(for members: [ClusterableLink]) -> String {
        let stems = members.map { stripExtension($0.filename) }
        guard !stems.isEmpty else { return "Package" }

        if let seasonPrefix = seasonEpisodePrefix(for: stems) {
            return beautify(seasonPrefix)
        }

        guard var prefix = stems.first else { return members.first?.host ?? "Package" }
        for stem in stems.dropFirst() {
            prefix = commonPrefix(prefix, stem)
            if prefix.isEmpty { break }
        }
        let cleaned = prefix.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
        guard !cleaned.isEmpty else { return members.first?.host ?? "Package" }
        return beautify(cleaned)
    }

    private static func commonPrefix(_ a: String, _ b: String) -> String {
        var result = ""
        for (charA, charB) in zip(a, b) {
            guard charA == charB else { break }
            result.append(charA)
        }
        return result
    }

    /// Detects a shared "S01E0x" style season/episode structure across
    /// every member's stem and, when found, returns the name up through the
    /// season token — "Show.S01E01" and "Show.S01E02" name as "Show.S01",
    /// not the meaningless "Show.S01E0" a naive character-by-character
    /// common prefix produces, since that stops the instant the episode
    /// digits diverge, mid-token.
    private static func seasonEpisodePrefix(for stems: [String]) -> String? {
        let pattern = #"(?i)^(.*?)(s\d{1,2})e\d{1,2}.*$"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }

        var sharedLeading: String?
        var sharedSeason: String?
        for stem in stems {
            let range = NSRange(stem.startIndex..<stem.endIndex, in: stem)
            guard let match = regex.firstMatch(in: stem, range: range),
                let leadingRange = Range(match.range(at: 1), in: stem),
                let seasonRange = Range(match.range(at: 2), in: stem)
            else { return nil }
            let leading = String(stem[leadingRange])
            let season = String(stem[seasonRange]).uppercased()
            if let currentLeading = sharedLeading, let currentSeason = sharedSeason {
                guard currentLeading.caseInsensitiveCompare(leading) == .orderedSame,
                    currentSeason == season
                else { return nil }
            } else {
                sharedLeading = leading
                sharedSeason = season
            }
        }
        guard let sharedLeading, let sharedSeason else { return nil }
        return sharedLeading + sharedSeason
    }

    /// Replaces `.`/`_`/`-` separators with spaces and collapses runs of
    /// them, so a package name reads like a title instead of a filename.
    /// Only capitalizes words that are entirely lowercase — "1080p",
    /// "BluRay", and "NASA" are left exactly as the source spelled them,
    /// since re-casing an already-mixed-case or all-caps word is as likely
    /// to mangle it as improve it.
    private static func beautify(_ raw: String) -> String {
        let separators = CharacterSet(charactersIn: "._-")
        let words = raw.components(separatedBy: separators).filter { !$0.isEmpty }
        let capitalized = words.map { word -> String in
            guard word == word.lowercased() else { return word }
            return word.prefix(1).uppercased() + word.dropFirst()
        }
        return capitalized.joined(separator: " ")
    }
}
