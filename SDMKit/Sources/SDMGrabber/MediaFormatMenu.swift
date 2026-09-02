import Foundation
import SDMCore
import SDMResolve

/// One selectable entry in a media row's format picker. Parent spec §9.2.
public struct MediaFormatOption: Identifiable, Sendable, Equatable {
    public let id: String
    public var label: String
    public var choice: FormatChoice
    public var matchesPreferences: Bool
}

/// Pure builder of the flat, match-first format-picker list. Matching
/// options (pass every allowlist + max-res filter) come first, then
/// non-matching; each group is ranked by `FormatSelector.videoRankLess`,
/// so the auto-pick is always the first matching entry.
public enum MediaFormatMenu {
    public static func options(
        for media: ResolvedMedia, preferences: QualityPreferences
    ) -> [MediaFormatOption] {
        let audioOnly = media.formats.filter { $0.kind == .audioOnly }
        let bestEligibleAudio = FormatSelector.rankedAudioFormats(media, preferences).first
        let bestAnyAudio =
            audioOnly.sorted { ($0.tbr ?? 0) > ($1.tbr ?? 0) }.first

        var options: [MediaFormatOption] = []

        for video in media.formats where video.kind == .progressive {
            let matches =
                (video.height ?? 0) <= preferences.maxHeight
                && video.vcodec.map { preferences.videoCodecs.contains($0) } == true
                && preferences.containers.contains(video.container)
            options.append(
                option(video: video, audio: nil, matches: matches))
        }

        for video in media.formats where video.kind == .videoOnly {
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
        return ranked
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
