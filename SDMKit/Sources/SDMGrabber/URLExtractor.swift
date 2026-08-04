import Foundation

/// Pulls `http`/`https` links out of arbitrary pasted or dropped text.
/// Spec §7.1: `NSDataDetector` rather than a regex, so URLs embedded in
/// prose and wrapped by ordinary line breaks are found correctly.
public enum URLExtractor {
    public static func extractLinks(from text: String) -> [URL] {
        guard
            let detector = try? NSDataDetector(
                types: NSTextCheckingResult.CheckingType.link.rawValue
            )
        else { return [] }

        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        var seen = Set<URL>()
        var result: [URL] = []

        detector.enumerateMatches(in: text, options: [], range: range) { match, _, _ in
            guard let url = match?.url, let scheme = url.scheme?.lowercased(),
                scheme == "http" || scheme == "https"
            else { return }
            if seen.insert(url).inserted { result.append(url) }
        }

        return result
    }
}
