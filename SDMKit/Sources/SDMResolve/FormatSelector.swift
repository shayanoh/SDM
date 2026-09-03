import Foundation
import SDMCore

/// Pure selection of the best format(s) for a resolved video given the
/// user's quality preferences. Fixture-tested, no I/O. Parent spec §4.3.
public enum FormatSelector {
    public static func pick(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        pickDirect(media, prefs) ?? pickWholesale(media, prefs)
    }

    /// The yt-dlp `-f` expression a wholesale (HLS/DASH) download uses. Leans
    /// on yt-dlp's own selection rather than pinning one manifest variant.
    public static func wholesaleSelector(maxHeight: Int) -> String {
        "bv*[height<=\(maxHeight)]+ba/b[height<=\(maxHeight)]/bv*+ba/b"
    }

    private static func pickDirect(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        let video = rankedVideoFormats(media, prefs).first
        let hasAnyVideo = media.formats.contains {
            ($0.kind == .videoOnly || $0.kind == .progressive) && $0.isDirect
        }

        if let video {
            if video.kind == .progressive {
                return FormatChoice(
                    video: video, audio: nil, outputContainer: video.container,
                    estimatedBytes: video.filesizeEffective)
            }
            guard let audio = rankedAudioFormats(media, prefs).first else { return nil }
            return FormatChoice(
                video: video, audio: audio, outputContainer: video.container,
                estimatedBytes: sumSize(video, audio))
        }

        // No eligible video. Only fall back to audio-only when the media
        // genuinely has no video-bearing formats at all.
        guard !hasAnyVideo, let audio = rankedAudioFormats(media, prefs).first else {
            return nil
        }
        return FormatChoice(
            video: nil, audio: audio, outputContainer: audio.container,
            estimatedBytes: audio.filesizeEffective)
    }

    /// When no `.direct` format fits, fall back to the best HLS/DASH variant
    /// (spec §6.3). Prefers video-bearing streams that fit `maxHeight`, but
    /// still returns something for a too-tall-only or audio-only stream.
    private static func pickWholesale(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        let streamed = media.formats.filter { !$0.isDirect }
        guard !streamed.isEmpty else { return nil }

        let videoish = streamed.filter {
            $0.kind == .videoOnly || $0.kind == .progressive
        }
        let pool = videoish.isEmpty ? streamed : videoish
        let underCap = pool.filter { ($0.height ?? 0) <= prefs.maxHeight }
        let candidates = underCap.isEmpty ? pool : underCap
        guard
            let best = candidates.max(by: {
                (($0.height ?? 0), ($0.tbr ?? 0)) < (($1.height ?? 0), ($1.tbr ?? 0))
            })
        else { return nil }

        let allWebm = candidates.allSatisfy { $0.container == .webm }
        return FormatChoice(
            video: best, audio: nil,
            outputContainer: allWebm ? .webm : .mp4,
            estimatedBytes: best.filesizeEffective,
            wholesaleSelector: wholesaleSelector(maxHeight: prefs.maxHeight))
    }

    public static func rankedVideoFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                format.isDirect
                    && (format.kind == .videoOnly || format.kind == .progressive)
                    && (format.height ?? 0) <= prefs.maxHeight
                    && format.vcodec.map { prefs.videoCodecs.contains($0) } == true
                    && prefs.containers.contains(format.container)
            }
            .sorted(by: videoRankLess)
    }

    public static func rankedAudioFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                format.isDirect && format.kind == .audioOnly
                    && format.acodec.map { prefs.audioCodecs.contains($0) } == true
            }
            .sorted { a, b in
                let ra = a.acodec?.rank ?? Int.max
                let rb = b.acodec?.rank ?? Int.max
                if ra != rb { return ra < rb }
                return (a.tbr ?? 0) > (b.tbr ?? 0)
            }
    }

    /// Higher resolution first, then codec priority, then container
    /// priority, then higher bitrate. Shared with the picker UI so the
    /// auto-pick is always the first matching row in the list.
    public static func videoRankLess(_ a: MediaFormat, _ b: MediaFormat) -> Bool {
        if (a.height ?? 0) != (b.height ?? 0) { return (a.height ?? 0) > (b.height ?? 0) }
        let ca = a.vcodec?.rank ?? Int.max
        let cb = b.vcodec?.rank ?? Int.max
        if ca != cb { return ca < cb }
        if a.container.rank != b.container.rank { return a.container.rank < b.container.rank }
        return (a.tbr ?? 0) > (b.tbr ?? 0)
    }

    private static func sumSize(_ a: MediaFormat, _ b: MediaFormat) -> Int64? {
        guard let sa = a.filesizeEffective, let sb = b.filesizeEffective else { return nil }
        return sa + sb
    }
}
