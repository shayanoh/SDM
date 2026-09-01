import Foundation
import SDMCore

struct YtDlpDump: Decodable {
    var id: String?
    var title: String?
    var duration: Double?
    var extractor: String?
    var _type: String?
    var formats: [YtDlpFormat]?
    var entries: [YtDlpEntry]?
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
    var ieKey: String?

    enum CodingKeys: String, CodingKey {
        case id, title, url
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

    static func hasDirectURL(_ f: YtDlpFormat) -> Bool {
        guard let url = f.url, !url.isEmpty else { return false }
        switch f.proto {
        case nil, "https", "http", "https_native": return true
        default: return false
        }
    }

    static func mediaFormat(from f: YtDlpFormat) -> MediaFormat? {
        guard let id = f.formatID, hasDirectURL(f),
            let urlString = f.url, let url = URL(string: urlString),
            (f.ext ?? "") != "mhtml"
        else { return nil }

        let vcodec = codecFor(vcodec: f.vcodec)
        let acodec = codecFor(acodec: f.acodec)
        let kind: MediaKind
        switch (vcodec, acodec) {
        case (.some, .some): kind = .progressive
        case (.some, .none): kind = .videoOnly
        case (.none, .some): kind = .audioOnly
        case (.none, .none): return nil
        }

        return MediaFormat(
            id: id, kind: kind, height: f.height, width: f.width,
            vcodec: vcodec, acodec: acodec, container: container(ext: f.ext),
            filesize: f.filesize, filesizeApprox: f.filesizeApprox, tbr: f.tbr, url: url)
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

    static func flatEntries(from dump: YtDlpDump) -> [(videoID: String, title: String)] {
        (dump.entries ?? []).compactMap { entry in
            guard let id = entry.id else { return nil }
            return (id, entry.title ?? id)
        }
    }
}
