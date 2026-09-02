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
            guard let url = match?.url, isGrabbable(url) else { return }
            if seen.insert(url).inserted { result.append(url) }
        }

        return result
    }

    /// Whether a URL is worth adding to the grabber: an `http`/`https` URL
    /// that actually points *at* something. A bare host with no path (or
    /// just `/`) and no query — `something.com`, `https://something.com/`,
    /// or a dotted token like a version string that `NSDataDetector`
    /// mistook for a link — is not a download and is dropped. A real path
    /// segment (`/file.zip`, `/watch`, `/@channel`) or any query string
    /// keeps it.
    public static func isGrabbable(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
            let host = url.host, !host.isEmpty
        else { return false }
        let path = url.path
        if !path.isEmpty && path != "/" { return true }
        let query = URLComponents(url: url, resolvingAgainstBaseURL: false)?.query
        return !(query ?? "").isEmpty
    }
}
