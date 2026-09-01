import Foundation
import SDMCore

/// Pure selection of the best format(s) for a resolved video given the
/// user's quality preferences. Fixture-tested, no I/O. Parent spec §4.3.
public enum FormatSelector {
    public static func pick(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        let video = rankedVideoFormats(media, prefs).first
        let hasAnyVideo = media.formats.contains {
            $0.kind == .videoOnly || $0.kind == .progressive
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

    public static func rankedVideoFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                (format.kind == .videoOnly || format.kind == .progressive)
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
                format.kind == .audioOnly
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
