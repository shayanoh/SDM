import Foundation
import SDMCore

/// Builds the one-line media description shown in the details panel for a
/// resolver-backed download: delivery, resolution, codecs, container,
/// bitrate, duration, and the source site. Pure. Parent spec
/// `2026-09-03-multi-site-resolver-design.md`.
public enum MediaMetadata {
    public static func describe(choice: FormatChoice, media: ResolvedMedia) -> String {
        var parts: [String] = []

        parts.append(choice.isWholesale ? "Streaming" : "Direct")

        if let height = choice.video?.height {
            parts.append("\(height)p")
        }
        if let vcodec = choice.video?.vcodec {
            parts.append(describe(vcodec))
        }
        if let acodec = choice.audio?.acodec ?? choice.video?.acodec {
            parts.append(describe(acodec))
        }

        parts.append(choice.outputContainer.fileExtension)

        if let bitrate = bitrate(choice) {
            parts.append(bitrate)
        }
        if let duration = duration(media.durationSeconds) {
            parts.append(duration)
        }
        let site = media.extractor.lowercased()
        if !site.isEmpty, site != "unknown", site != "generic" {
            parts.append(site)
        }

        return parts.joined(separator: " · ")
    }

    /// yt-dlp `tbr` is in kbps; sum video + audio when muxing.
    private static func bitrate(_ choice: FormatChoice) -> String? {
        let total = (choice.video?.tbr ?? 0) + (choice.audio?.tbr ?? 0)
        guard total > 0 else { return nil }
        if total >= 1000 {
            return String(format: "%.1f Mbps", total / 1000)
        }
        return "\(Int(total.rounded())) kbps"
    }

    private static func duration(_ seconds: Double?) -> String? {
        guard let seconds, seconds >= 1 else { return nil }
        let total = Int(seconds.rounded())
        let h = total / 3600
        let m = (total % 3600) / 60
        let s = total % 60
        return h > 0
            ? String(format: "%d:%02d:%02d", h, m, s)
            : String(format: "%d:%02d", m, s)
    }

    private static func describe(_ codec: VideoCodec) -> String {
        switch codec {
        case .av1: return "av1"
        case .vp9: return "vp9"
        case .h264: return "h264"
        case .other(let raw): return raw
        }
    }

    private static func describe(_ codec: AudioCodec) -> String {
        switch codec {
        case .opus: return "opus"
        case .aac: return "aac"
        case .other(let raw): return raw
        }
    }
}
