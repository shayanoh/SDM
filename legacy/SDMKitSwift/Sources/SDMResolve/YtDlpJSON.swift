import Foundation
import SDMCore

struct YtDlpDump: Decodable {
    var id: String?
    var title: String?
    var duration: Double?
    var extractor: String?
    var type: String?
    var formats: [YtDlpFormat]?
    var entries: [YtDlpEntry]?

    enum CodingKeys: String, CodingKey {
        case id, title, duration, extractor, formats, entries
        case type = "_type"
    }
}

struct YtDlpFormat: Decodable {
    var formatID: String?
    var ext: String?
    var vcodec: String?
    var acodec: String?
    var height: Int?
    var width: Int?
    var filesize: Int64?
    var filesizeApprox: Int64?
    var tbr: Double?
    var url: String?
    var proto: String?

    enum CodingKeys: String, CodingKey {
        case formatID = "format_id"
        case ext, vcodec, acodec, height, width, filesize
        case filesizeApprox = "filesize_approx"
        case tbr, url
        case proto = "protocol"
    }
}

struct YtDlpEntry: Decodable {
    var id: String?
    var title: String?
    var url: String?
    var webpageURL: String?
    var ieKey: String?

    enum CodingKeys: String, CodingKey {
        case id, title, url
        case webpageURL = "webpage_url"
        case ieKey = "ie_key"
    }
}

enum YtDlpParser {
    static func codecFor(vcodec raw: String?) -> VideoCodec? {
        guard let raw, !raw.isEmpty, raw != "none" else { return nil }
        let lower = raw.lowercased()
        if lower.hasPrefix("av01") || lower.hasPrefix("av1") { return .av1 }
        if lower.hasPrefix("vp9") || lower.hasPrefix("vp09") { return .vp9 }
        if lower.hasPrefix("avc1") || lower.hasPrefix("h264") { return .h264 }
        return .other(raw)
    }

    static func codecFor(acodec raw: String?) -> AudioCodec? {
        guard let raw, !raw.isEmpty, raw != "none" else { return nil }
        let lower = raw.lowercased()
        if lower.hasPrefix("opus") { return .opus }
        if lower.hasPrefix("mp4a") || lower.hasPrefix("aac") { return .aac }
        return .other(raw)
    }

    static func container(ext raw: String?) -> MediaContainer {
        switch (raw ?? "").lowercased() {
        case "mp4", "m4v": return .mp4
        case "webm": return .webm
        case "m4a": return .m4a
        case "": return .other("bin")
        default: return .other(raw!)
        }
    }

    static func isVideoContainer(_ ext: String?) -> Bool {
        ["mp4", "m4v", "webm", "mkv", "mov", "flv", "ts", "avi", "3gp"]
            .contains((ext ?? "").lowercased())
    }

    static func isAudioContainer(_ ext: String?) -> Bool {
        ["m4a", "mp3", "aac", "opus", "ogg", "oga", "flac", "wav"]
            .contains((ext ?? "").lowercased())
    }

    /// yt-dlp `protocol` → how the engine must fetch it. `nil` ⇒ this
    /// format is not usable (rtmp, ism, mss, …). Parent spec §6.1.
    static func deliveryFor(proto raw: String?) -> MediaDelivery? {
        switch (raw ?? "").lowercased() {
        case "", "https", "http", "https_native": return .direct
        case "m3u8", "m3u8_native": return .hls
        case "http_dash_segments": return .dash
        default: return nil
        }
    }

    static func mediaFormat(from f: YtDlpFormat) -> MediaFormat? {
        guard let id = f.formatID,
            let urlString = f.url, !urlString.isEmpty, let url = URL(string: urlString),
            (f.ext ?? "") != "mhtml",
            let delivery = deliveryFor(proto: f.proto)
        else { return nil }

        let vcodec = codecFor(vcodec: f.vcodec)
        let acodec = codecFor(acodec: f.acodec)
        let kind: MediaKind
        switch (vcodec, acodec) {
        case (.some, .some): kind = .progressive
        case (.some, .none): kind = .videoOnly
        case (.none, .some): kind = .audioOnly
        case (.none, .none):
            // Some extractors (xnxx, xvideos, …) omit codec info even for
            // fully downloadable muxed streams. Infer the kind rather than
            // dropping the format: a real resolution ⇒ a video variant; a
            // direct single-file video container ⇒ a muxed stream. A
            // codec-less HLS entry with no resolution is ambiguous (often a
            // bare audio manifest, e.g. YouTube 233/234) — skip it.
            if f.height != nil {
                kind = .progressive
            } else if delivery == .direct && isVideoContainer(f.ext) {
                kind = .progressive
            } else if isAudioContainer(f.ext)
                || (f.formatID ?? "").lowercased().contains("audio")
            {
                kind = .audioOnly
            } else {
                return nil
            }
        }

        return MediaFormat(
            id: id, kind: kind, height: f.height, width: f.width,
            vcodec: vcodec, acodec: acodec, container: container(ext: f.ext),
            filesize: f.filesize, filesizeApprox: f.filesizeApprox, tbr: f.tbr, url: url,
            delivery: delivery)
    }

    static func resolvedMedia(from dump: YtDlpDump) throws -> ResolvedMedia {
        guard let id = dump.id, let rawFormats = dump.formats, !rawFormats.isEmpty else {
            throw ResolveError.unavailable
        }
        let formats = rawFormats.compactMap(mediaFormat(from:))
        guard !formats.isEmpty else { throw ResolveError.unsupported }
        return ResolvedMedia(
            extractor: dump.extractor ?? "unknown",
            videoID: id,
            title: dump.title ?? id,
            durationSeconds: dump.duration,
            formats: formats)
    }

    /// Playlist / channel entries with their own page URL. `entry.url` (the
    /// `--flat-playlist` field) is preferred, then `webpage_url` (present in
    /// a full `-J` playlist dump); relative values are resolved against the
    /// playlist URL. Entries with no usable URL are dropped. Parent spec §5.3.
    static func flatEntries(
        from dump: YtDlpDump, relativeTo base: URL
    ) -> [(sourceURL: URL, videoID: String, title: String)] {
        (dump.entries ?? []).compactMap { entry in
            let raw = entry.url ?? entry.webpageURL
            guard let raw, !raw.isEmpty,
                let resolved = URL(string: raw, relativeTo: base)?.absoluteURL
            else { return nil }
            let id = entry.id ?? resolved.lastPathComponent
            return (resolved, id, entry.title ?? id)
        }
    }
}
