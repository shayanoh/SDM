import Foundation

/// Mines scene / release quality tags out of a download's filename
/// (`The.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GRP.mkv` →
/// `1080p · WEB-DL · H.264 · DDP · 5.1`). Pure and fixture-tested like
/// `VerdictRules` — tune the token table, not the control flow.
public enum ReleaseTags {
    /// Canonical group order for the output string.
    enum Category: Int, CaseIterable {
        case resolution, source, remux, platform, hdr, videoCodec, audioCodec,
            channels, edition
    }

    /// `(match token, display, category)`. Tokens are lowercased; every
    /// separator except `+` is normalised to `.` in both the token and the
    /// filename, and a token matches only on `.` boundaries — plus a
    /// `<token><digit>` form so `ddp` catches `DDP5.1`. Only `resolution`
    /// keeps the first hit; other categories accumulate real combos
    /// (`HDR10 · DV`, `TrueHD · Atmos`, `BluRay · REMUX`).
    static let table: [(token: String, display: String, category: Category)] = [
        // resolution
        ("2160p", "2160p", .resolution), ("1440p", "1440p", .resolution),
        ("1080p", "1080p", .resolution), ("720p", "720p", .resolution),
        ("576p", "576p", .resolution), ("480p", "480p", .resolution),
        ("360p", "360p", .resolution), ("uhd", "UHD", .resolution),
        ("4k", "4K", .resolution),
        // source
        ("web.dl", "WEB-DL", .source), ("webdl", "WEB-DL", .source),
        ("webrip", "WEBRip", .source), ("web", "WEB", .source),
        ("bluray", "BluRay", .source), ("blu.ray", "BluRay", .source),
        ("bdrip", "BDRip", .source), ("brrip", "BRRip", .source),
        ("hdtv", "HDTV", .source), ("pdtv", "PDTV", .source),
        ("dvdrip", "DVDRip", .source), ("dvdscr", "DVDScr", .source),
        ("hdrip", "HDRip", .source), ("hdcam", "HDCAM", .source),
        ("hdts", "HDTS", .source), ("cam", "CAM", .source),
        ("telesync", "TELESYNC", .source),
        // remux
        ("bdremux", "BD-REMUX", .remux), ("remux", "REMUX", .remux),
        // streaming platform
        ("amzn", "AMZN", .platform), ("nf", "NF", .platform),
        ("dsnp", "DSNP", .platform), ("hmax", "HMAX", .platform),
        ("atvp", "ATVP", .platform), ("hulu", "HULU", .platform),
        ("pcok", "PCOK", .platform), ("stan", "STAN", .platform),
        // hdr / bit depth
        ("hdr10plus", "HDR10+", .hdr), ("hdr10+", "HDR10+", .hdr),
        ("hdr10", "HDR10", .hdr), ("hdr", "HDR", .hdr),
        ("dovi", "DoVi", .hdr), ("dv", "DV", .hdr), ("hlg", "HLG", .hdr),
        ("10bit", "10bit", .hdr),
        // video codec
        ("x265", "x265", .videoCodec), ("h.265", "H.265", .videoCodec),
        ("h265", "H.265", .videoCodec), ("hevc", "HEVC", .videoCodec),
        ("x264", "x264", .videoCodec), ("h.264", "H.264", .videoCodec),
        ("h264", "H.264", .videoCodec), ("avc", "AVC", .videoCodec),
        ("av1", "AV1", .videoCodec), ("xvid", "XviD", .videoCodec),
        ("divx", "DivX", .videoCodec), ("vp9", "VP9", .videoCodec),
        // audio codec
        ("truehd", "TrueHD", .audioCodec), ("atmos", "Atmos", .audioCodec),
        ("dts.hd", "DTS-HD", .audioCodec), ("dtshd", "DTS-HD", .audioCodec),
        ("dts.x", "DTS:X", .audioCodec), ("dts", "DTS", .audioCodec),
        ("eac3", "EAC3", .audioCodec), ("dd+", "DDP", .audioCodec),
        ("ddp", "DDP", .audioCodec), ("ac3", "AC3", .audioCodec),
        ("dd", "DD", .audioCodec), ("aac", "AAC", .audioCodec),
        ("flac", "FLAC", .audioCodec), ("opus", "Opus", .audioCodec),
        ("mp3", "MP3", .audioCodec), ("ac4", "AC4", .audioCodec),
        // channel layout
        ("7.1", "7.1", .channels), ("5.1", "5.1", .channels),
        ("2.0", "2.0", .channels),
        // edition
        ("proper", "PROPER", .edition), ("repack", "REPACK", .edition),
        ("extended", "EXTENDED", .edition), ("uncut", "UNCUT", .edition),
        ("remastered", "REMASTERED", .edition), ("imax", "IMAX", .edition),
        ("directors.cut", "Director's Cut", .edition),
    ]

    /// `nil` when the filename yields no recognizable tag.
    public static func extract(from filename: String) -> String? {
        let stem = (filename as NSString).deletingPathExtension.lowercased()
        var normalised = ""
        for scalar in stem.unicodeScalars {
            if CharacterSet(charactersIn: " _-[](){},'").contains(scalar) {
                normalised.append(".")
            } else {
                normalised.unicodeScalars.append(scalar)
            }
        }
        while normalised.contains("..") {
            normalised = normalised.replacingOccurrences(of: "..", with: ".")
        }
        let hay = "." + normalised.trimmingCharacters(in: CharacterSet(charactersIn: ".")) + "."

        var chosen: [Category: [String]] = [:]
        for entry in table {
            guard matches(entry.token, category: entry.category, in: hay) else { continue }
            var list = chosen[entry.category] ?? []
            if entry.category == .resolution {
                if list.isEmpty { list = [entry.display] }
            } else if !list.contains(entry.display) {
                list.append(entry.display)
            }
            chosen[entry.category] = list
        }
        // `web` fires alongside `web.dl` / `webrip`; drop the redundant bare tag.
        if var source = chosen[.source], source.count > 1 {
            source.removeAll { $0 == "WEB" }
            chosen[.source] = source
        }
        guard !chosen.isEmpty else { return nil }
        return Category.allCases.flatMap { chosen[$0] ?? [] }.joined(separator: " · ")
    }

    private static func matches(
        _ token: String, category: Category, in hay: String
    ) -> Bool {
        // Channel layouts glue onto the codec (`DDP5.1`), so match `5.1.`
        // anywhere rather than only on a leading boundary.
        if category == .channels { return hay.contains(token + ".") }
        if hay.contains("." + token + ".") { return true }
        // Audio codecs glue onto the channel count (`DDP5.1`, `AAC2.0`), so
        // also accept `<token><digit>`. Other categories must land on a `.`
        // boundary (so `hdr` ≠ `hdr10`, `web` ≠ `webrip`).
        guard category == .audioCodec, let r = hay.range(of: "." + token) else {
            return false
        }
        return hay[r.upperBound...].first.map(\.isNumber) == true
    }
}
