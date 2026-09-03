import Foundation
import SDMCore
import SDMResolve

/// One selectable entry in a media row's format picker. Parent spec §9.2.
public struct MediaFormatOption: Identifiable, Sendable, Equatable {
    public let id: String
    public var label: String
    public var choice: FormatChoice
    public var matchesPreferences: Bool
    /// This option is an HLS/DASH stream that yt-dlp downloads wholesale
    /// (non-resumable). The picker badges it "streamed".
    public var isWholesale: Bool = false
}

/// Pure builder of the flat, match-first format-picker list. Matching
/// options (pass every allowlist + max-res filter) come first, then
/// non-matching; each group is ranked by `FormatSelector.videoRankLess`,
/// so the auto-pick is always the first matching entry.
public enum MediaFormatMenu {
    public static func options(
        for media: ResolvedMedia, preferences: QualityPreferences
    ) -> [MediaFormatOption] {
        let audioOnly = media.formats.filter { $0.kind == .audioOnly && $0.isDirect }
        let bestEligibleAudio = FormatSelector.rankedAudioFormats(media, preferences).first
        let bestAnyAudio =
            audioOnly.sorted { ($0.tbr ?? 0) > ($1.tbr ?? 0) }.first

        var options: [MediaFormatOption] = []

        // Only `.direct` formats get a plain-download row. HLS/DASH
        // progressive / video-only formats are handled by the `streamed`
        // section below — a "normal" row for one would point the engine at
        // an `.m3u8` playlist and save it verbatim as `.mp4`.
        for video in media.formats where video.kind == .progressive && video.isDirect {
            let matches =
                (video.height ?? 0) <= preferences.maxHeight
                && video.vcodec.map { preferences.videoCodecs.contains($0) } == true
                && preferences.containers.contains(video.container)
            options.append(
                option(video: video, audio: nil, matches: matches))
        }

        for video in media.formats where video.kind == .videoOnly && video.isDirect {
            let videoMatches =
                (video.height ?? 0) <= preferences.maxHeight
                && video.vcodec.map { preferences.videoCodecs.contains($0) } == true
                && preferences.containers.contains(video.container)
            let audio = bestEligibleAudio ?? bestAnyAudio
            guard let audio else { continue }
            let matches = videoMatches && (bestEligibleAudio != nil)
            options.append(option(video: video, audio: audio, matches: matches))
        }

        let ranked = options.sorted { lhs, rhs in
            if lhs.matchesPreferences != rhs.matchesPreferences {
                return lhs.matchesPreferences && !rhs.matchesPreferences
            }
            guard let lv = lhs.choice.video, let rv = rhs.choice.video else { return false }
            return FormatSelector.videoRankLess(lv, rv)
        }

        // HLS/DASH variants land after every direct option, tallest first.
        let streamed =
            media.formats
            .filter { !$0.isDirect && ($0.kind == .videoOnly || $0.kind == .progressive) }
            .sorted { ($0.height ?? 0) > ($1.height ?? 0) }
            .map { wholesaleOption(for: $0) }

        return ranked + streamed
    }

    private static func wholesaleOption(for format: MediaFormat) -> MediaFormatOption {
        let selector =
            format.kind == .progressive ? format.id : format.id + "+ba/b"
        let height = format.height.map { "\($0)p" } ?? "auto"
        return MediaFormatOption(
            id: "w:\(format.id)",
            label: "\(height) · streamed",
            choice: FormatChoice(
                video: format, audio: nil, outputContainer: .mp4,
                estimatedBytes: format.filesizeEffective, wholesaleSelector: selector),
            matchesPreferences: false,
            isWholesale: true)
    }

    private static func option(
        video: MediaFormat, audio: MediaFormat?, matches: Bool
    ) -> MediaFormatOption {
        let container = video.kind == .progressive ? video.container : video.container
        let estimated = combinedSize(video, audio)
        return MediaFormatOption(
            id: "\(video.id)+\(audio?.id ?? "-")",
            label: label(video: video, audio: audio, container: container, estimated: estimated),
            choice: FormatChoice(
                video: video, audio: video.kind == .progressive ? nil : audio,
                outputContainer: container, estimatedBytes: estimated),
            matchesPreferences: matches)
    }

    private static func combinedSize(_ video: MediaFormat, _ audio: MediaFormat?) -> Int64? {
        if video.kind == .progressive { return video.filesizeEffective }
        guard let v = video.filesizeEffective, let a = audio?.filesizeEffective else { return nil }
        return v + a
    }

    private static func label(
        video: MediaFormat, audio: MediaFormat?, container: MediaContainer, estimated: Int64?
    ) -> String {
        var parts: [String] = []
        if let height = video.height { parts.append("\(height)p") }
        if let vcodec = video.vcodec { parts.append(describe(vcodec)) }
        parts.append(container.fileExtension)
        let approximate =
            video.isApproximateSize || (audio?.isApproximateSize ?? false)
        if let estimated {
            let formatter = ByteCountFormatter()
            formatter.countStyle = .binary
            parts.append((approximate ? "~" : "") + formatter.string(fromByteCount: estimated))
        } else {
            parts.append("—")
        }
        return parts.joined(separator: " · ")
    }

    private static func describe(_ codec: VideoCodec) -> String {
        switch codec {
        case .av1: return "av1"
        case .vp9: return "vp9"
        case .h264: return "h264"
        case .other(let raw): return raw
        }
    }
}
