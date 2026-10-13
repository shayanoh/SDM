import Foundation
import SDMCore

/// Parses one line of `yt-dlp` output (as a wholesale downloader) into a
/// `WholesaleProgress`, or `nil` for lines that carry no progress signal.
///
/// Download lines come from
/// `--progress-template "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.fragment_index)s|%(progress.fragment_count)s"`.
///
/// For HLS/DASH downloads `total_bytes` is `NA`, `total_bytes_estimate` is a
/// float string, and the reliable progress signal is
/// `fragment_index / fragment_count`. Post-processing (merge / fixup /
/// extract-audio) is detected by yt-dlp's bracketed prefixes. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.6.
public enum WholesaleProgressParser {
    private static let postProcessMarkers = [
        "[merger]", "merging formats", "[extractaudio]", "[fixupm3u8]",
        "[videoconvertor]", "[fixup", "[videoremuxer]",
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
        guard fields.count >= 4 else { return nil }

        let downloaded = number(fields[1])
        let total = number(fields[2])
        let estimate = number(fields[3])

        var fraction: Double?
        var isFragmented = false
        if fields.count >= 6,
            let index = number(fields[4]), let count = number(fields[5]), count > 0
        {
            fraction = min(1, max(0, Double(index) / Double(count)))
            isFragmented = true
        }

        return WholesaleProgress(
            downloadedBytes: downloaded,
            totalBytes: total ?? estimate,
            fraction: fraction,
            phase: .downloading,
            isFragmented: isFragmented)
    }

    /// yt-dlp numbers arrive as integers, comma-grouped, or — for
    /// `total_bytes_estimate` on fragmented downloads — floats (`"1.3e10"` /
    /// `"13161769424.0"`). `NA` / `None` / empty → nil.
    private static func number(_ raw: String) -> Int64? {
        let cleaned = raw.replacingOccurrences(of: ",", with: "")
        guard !cleaned.isEmpty, cleaned.uppercased() != "NA", cleaned != "None"
        else { return nil }
        if let value = Int64(cleaned), value >= 0 { return value }
        if let value = Double(cleaned), value.isFinite, value >= 0 {
            return Int64(value.rounded())
        }
        return nil
    }
}
