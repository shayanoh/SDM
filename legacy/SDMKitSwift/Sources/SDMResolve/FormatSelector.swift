import Foundation
import SDMCore

/// Pure selection of the best format(s) for a resolved video given the
/// user's quality preferences. Fixture-tested, no I/O. Parent spec §4.3.
public enum FormatSelector {
    public static func pick(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        // Rank direct and HLS/DASH video formats together by quality, so a
        // 1080p HLS variant beats a metadata-less direct 360p (xnxx/xvideos)
        // while a real direct 1080p beats an equivalent HLS one.
        if let best = eligibleVideoFormats(media, prefs).first,
            let choice = videoChoice(for: best, media, prefs)
        {
            return choice
        }

        // Nothing fits the height ceiling. An HLS/DASH-only video still gets
        // its smallest variant downloaded wholesale (better than
        // "unsupported"); a direct video above the cap is left alone — the
        // user set the ceiling and a resumable download honors it.
        let smallestStreamed =
            media.formats
            .filter { !$0.isDirect && ($0.kind == .videoOnly || $0.kind == .progressive) }
            .min { ($0.height ?? .max) < ($1.height ?? .max) }
        if let smallestStreamed, let choice = videoChoice(for: smallestStreamed, media, prefs) {
            return choice
        }

        // Audio-only media — only when there is no video-bearing format at all.
        let hasAnyVideo = media.formats.contains {
            $0.kind == .videoOnly || $0.kind == .progressive
        }
        if !hasAnyVideo, let audio = rankedAudioFormats(media, prefs).first {
            return FormatChoice(
                video: nil, audio: audio, outputContainer: audio.container,
                estimatedBytes: audio.filesizeEffective)
        }
        return nil
    }

    /// The yt-dlp `-f` expression a wholesale (HLS/DASH) download uses. Leans
    /// on yt-dlp's own selection rather than pinning one manifest variant.
    public static func wholesaleSelector(maxHeight: Int) -> String {
        "bv*[height<=\(maxHeight)]+ba/b[height<=\(maxHeight)]/bv*+ba/b"
    }

    /// A concrete choice for `best`, or `nil` when it is a direct video-only
    /// stream with no eligible direct audio to mux against.
    private static func videoChoice(
        for best: MediaFormat, _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        if !best.isDirect {
            return FormatChoice(
                video: best, audio: nil,
                outputContainer: best.container == .webm ? .webm : .mp4,
                estimatedBytes: best.filesizeEffective,
                wholesaleSelector: wholesaleSelector(maxHeight: prefs.maxHeight))
        }
        if best.kind == .progressive {
            return FormatChoice(
                video: best, audio: nil, outputContainer: best.container,
                estimatedBytes: best.filesizeEffective)
        }
        guard let audio = rankedAudioFormats(media, prefs).first else { return nil }
        return FormatChoice(
            video: best, audio: audio, outputContainer: best.container,
            estimatedBytes: sumSize(best, audio))
    }

    /// Video-bearing formats (direct + HLS/DASH) that satisfy the codec /
    /// container allowlists and the max-height ceiling, best quality first.
    /// A `nil` codec is unknown, not disqualifying: many extractors omit it.
    static func eligibleVideoFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                (format.kind == .videoOnly || format.kind == .progressive)
                    && (format.height ?? 0) <= prefs.maxHeight
                    && (format.vcodec == nil
                        || prefs.videoCodecs.contains(format.vcodec!))
                    && prefs.containers.contains(format.container)
            }
            .sorted(by: videoRankLess)
    }

    public static func rankedVideoFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        eligibleVideoFormats(media, prefs).filter(\.isDirect)
    }

    public static func rankedAudioFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                format.isDirect && format.kind == .audioOnly
                    && (format.acodec == nil || prefs.audioCodecs.contains(format.acodec!))
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
        // At the same resolution a direct (resumable, no yt-dlp) stream wins
        // — even over a fancier-codec HLS/DASH one.
        if a.isDirect != b.isDirect { return a.isDirect }
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
