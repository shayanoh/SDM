import Foundation

private let mediaAndArchiveExtensions: Set<String> = [
    "mp4", "mkv", "avi", "mov", "m4v", "zip", "rar", "7z", "gz", "tar", "iso", "pdf",
]

private let suspiciousPathTokens = ["login", "error", "expired", "404", "not-found", "denied"]

/// Table-driven pure function over a finished probe. Spec §7.3.
public enum VerdictRules {
    public static func evaluate(_ link: ProbedLink) -> Verdict {
        guard !link.transportFailed else { return .checkFailed }
        guard let status = link.statusCode else { return .checkFailed }
        guard (200...299).contains(status) else { return .offline }

        let ext =
            link.effectiveFilename.split(separator: ".").last.map { String($0).lowercased() } ?? ""
        let isMediaOrArchive = mediaAndArchiveExtensions.contains(ext)

        if isMediaOrArchive, link.contentType?.lowercased() == "text/html" {
            return .faulty(reason: "html, not \(ext)")
        }

        if isMediaOrArchive, let length = link.contentLength, length < 1024 {
            return .faulty(reason: "too small to be a real \(ext)")
        }

        if let signature = link.sniffedSignature, !ext.isEmpty, !signature.matches(extension: ext) {
            return .faulty(reason: "file signature does not match .\(ext)")
        }

        if link.originalURL.host != link.finalURL.host {
            let path = link.finalURL.path.lowercased()
            if suspiciousPathTokens.contains(where: path.contains) {
                return .faulty(reason: "redirected to \(link.finalURL.host ?? "unknown host")")
            }
        }

        return .online
    }
}
