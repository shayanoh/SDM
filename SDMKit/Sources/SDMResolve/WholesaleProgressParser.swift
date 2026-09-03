import Foundation
import SDMCore

/// Parses one line of `yt-dlp` output (as a wholesale downloader) into a
/// `WholesaleProgress`, or `nil` for lines that carry no progress signal.
///
/// The download lines come from
/// `--progress-template "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress._percent_str)s"`.
/// Post-processing (merge / fixup / extract-audio) is detected by yt-dlp's
/// own bracketed prefixes. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.6.
public enum WholesaleProgressParser {
    private static let postProcessMarkers = [
        "[merger]", "merging formats", "[extractaudio]", "[fixupm3u8]",
        "[fixupm3u8]", "[videoconvertor]", "[fixup", "[videoremuxer]",
    ]

    public static func parse(_ line: String) -> WholesaleProgress? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.hasPrefix("sdm:") {
            return parseDownloadLine(String(trimmed.dropFirst(4)))
        }
        let lower = trimmed.lowercased()
        if postProcessMarkers.contains(where: { lower.contains($0) }) {
            return WholesaleProgress(phase: .postProcessing)
        }
        return nil
    }

    private static func parseDownloadLine(_ body: String) -> WholesaleProgress? {
        let fields = body.split(separator: "|", omittingEmptySubsequences: false).map {
            String($0).trimmingCharacters(in: .whitespaces)
        }
        guard fields.count >= 5 else { return nil }

        let downloaded = int(fields[1])
        let total = int(fields[2])
        let estimate = int(fields[3])
        let fraction = percent(fields[4])

        return WholesaleProgress(
            downloadedBytes: downloaded,
            totalBytes: total ?? estimate,
            fraction: fraction,
            phase: .downloading)
    }

    private static func int(_ raw: String) -> Int64? {
        let cleaned = raw.replacingOccurrences(of: ",", with: "")
        guard !cleaned.isEmpty, cleaned.uppercased() != "NA", cleaned != "None",
            let value = Int64(cleaned), value >= 0
        else { return nil }
        return value
    }

    private static func percent(_ raw: String) -> Double? {
        let cleaned = raw.replacingOccurrences(of: "%", with: "")
            .trimmingCharacters(in: .whitespaces)
        guard let value = Double(cleaned), value.isFinite, value >= 0 else { return nil }
        return value / 100
    }
}
