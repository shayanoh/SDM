/// Mines scene / release quality tags out of a download's filename
/// (`The.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GRP.mkv` →
/// `1080p · WEB-DL · H.264 · DDP · 5.1`). Pure and fixture-tested like
/// `VerdictRules` — tune the token table, not the control flow.
pub enum ReleaseTags {}

/// Canonical group order for the output string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Resolution,
    Source,
    Remux,
    Platform,
    Hdr,
    VideoCodec,
    AudioCodec,
    Channels,
    Edition,
}

const CATEGORIES: [Category; 9] = [
    Category::Resolution,
    Category::Source,
    Category::Remux,
    Category::Platform,
    Category::Hdr,
    Category::VideoCodec,
    Category::AudioCodec,
    Category::Channels,
    Category::Edition,
];

use Category::*;

/// `(match token, display, category)`. Tokens are lowercased; every
/// separator except `+` is normalised to `.` in both the token and the
/// filename, and a token matches only on `.` boundaries — plus a
/// `<token><digit>` form so `ddp` catches `DDP5.1`. Only `resolution`
/// keeps the first hit; other categories accumulate real combos
/// (`HDR10 · DV`, `TrueHD · Atmos`, `BluRay · REMUX`).
const TABLE: &[(&str, &str, Category)] = &[
    // resolution
    ("2160p", "2160p", Resolution),
    ("1440p", "1440p", Resolution),
    ("1080p", "1080p", Resolution),
    ("720p", "720p", Resolution),
    ("576p", "576p", Resolution),
    ("480p", "480p", Resolution),
    ("360p", "360p", Resolution),
    ("uhd", "UHD", Resolution),
    ("4k", "4K", Resolution),
    // source
    ("web.dl", "WEB-DL", Source),
    ("webdl", "WEB-DL", Source),
    ("webrip", "WEBRip", Source),
    ("web", "WEB", Source),
    ("bluray", "BluRay", Source),
    ("blu.ray", "BluRay", Source),
    ("bdrip", "BDRip", Source),
    ("brrip", "BRRip", Source),
    ("hdtv", "HDTV", Source),
    ("pdtv", "PDTV", Source),
    ("dvdrip", "DVDRip", Source),
    ("dvdscr", "DVDScr", Source),
    ("hdrip", "HDRip", Source),
    ("hdcam", "HDCAM", Source),
    ("hdts", "HDTS", Source),
    ("cam", "CAM", Source),
    ("telesync", "TELESYNC", Source),
    // remux
    ("bdremux", "BD-REMUX", Remux),
    ("remux", "REMUX", Remux),
    // streaming platform
    ("amzn", "AMZN", Platform),
    ("nf", "NF", Platform),
    ("dsnp", "DSNP", Platform),
    ("hmax", "HMAX", Platform),
    ("atvp", "ATVP", Platform),
    ("hulu", "HULU", Platform),
    ("pcok", "PCOK", Platform),
    ("stan", "STAN", Platform),
    // hdr / bit depth
    ("hdr10plus", "HDR10+", Hdr),
    ("hdr10+", "HDR10+", Hdr),
    ("hdr10", "HDR10", Hdr),
    ("hdr", "HDR", Hdr),
    ("dovi", "DoVi", Hdr),
    ("dv", "DV", Hdr),
    ("hlg", "HLG", Hdr),
    ("10bit", "10bit", Hdr),
    // video codec
    ("x265", "x265", VideoCodec),
    ("h.265", "H.265", VideoCodec),
    ("h265", "H.265", VideoCodec),
    ("hevc", "HEVC", VideoCodec),
    ("x264", "x264", VideoCodec),
    ("h.264", "H.264", VideoCodec),
    ("h264", "H.264", VideoCodec),
    ("avc", "AVC", VideoCodec),
    ("av1", "AV1", VideoCodec),
    ("xvid", "XviD", VideoCodec),
    ("divx", "DivX", VideoCodec),
    ("vp9", "VP9", VideoCodec),
    // audio codec
    ("truehd", "TrueHD", AudioCodec),
    ("atmos", "Atmos", AudioCodec),
    ("dts.hd", "DTS-HD", AudioCodec),
    ("dtshd", "DTS-HD", AudioCodec),
    ("dts.x", "DTS:X", AudioCodec),
    ("dts", "DTS", AudioCodec),
    ("eac3", "EAC3", AudioCodec),
    ("dd+", "DDP", AudioCodec),
    ("ddp", "DDP", AudioCodec),
    ("ac3", "AC3", AudioCodec),
    ("dd", "DD", AudioCodec),
    ("aac", "AAC", AudioCodec),
    ("flac", "FLAC", AudioCodec),
    ("opus", "Opus", AudioCodec),
    ("mp3", "MP3", AudioCodec),
    ("ac4", "AC4", AudioCodec),
    // channel layout
    ("7.1", "7.1", Channels),
    ("5.1", "5.1", Channels),
    ("2.0", "2.0", Channels),
    // edition
    ("proper", "PROPER", Edition),
    ("repack", "REPACK", Edition),
    ("extended", "EXTENDED", Edition),
    ("uncut", "UNCUT", Edition),
    ("remastered", "REMASTERED", Edition),
    ("imax", "IMAX", Edition),
    ("directors.cut", "Director's Cut", Edition),
];

/// The characters `extract` turns into `.` (Swift's
/// `CharacterSet(charactersIn: " _-[](){},'")`).
const SEPARATORS: &str = " _-[](){},'";

impl ReleaseTags {
    /// `None` when the filename yields no recognizable tag.
    pub fn extract(filename: &str) -> Option<String> {
        let stem = deleting_path_extension(filename).to_lowercase();
        let mut normalised: String = stem
            .chars()
            .map(|c| if SEPARATORS.contains(c) { '.' } else { c })
            .collect();
        while normalised.contains("..") {
            normalised = normalised.replace("..", ".");
        }
        let hay = format!(".{}.", normalised.trim_matches('.'));

        let mut chosen: Vec<Vec<&str>> = vec![Vec::new(); CATEGORIES.len()];
        for &(token, display, category) in TABLE {
            if !matches(token, category, &hay) {
                continue;
            }
            let list = &mut chosen[category as usize];
            if category == Resolution {
                if list.is_empty() {
                    list.push(display);
                }
            } else if !list.contains(&display) {
                list.push(display);
            }
        }
        // `web` fires alongside `web.dl` / `webrip`; drop the redundant bare tag.
        let source = &mut chosen[Source as usize];
        if source.len() > 1 {
            source.retain(|d| *d != "WEB");
        }
        if chosen.iter().all(Vec::is_empty) {
            return None;
        }
        Some(
            CATEGORIES
                .iter()
                .flat_map(|c| chosen[*c as usize].iter().copied())
                .collect::<Vec<_>>()
                .join(" · "),
        )
    }
}

fn matches(token: &str, category: Category, hay: &str) -> bool {
    // Channel layouts glue onto the codec (`DDP5.1`), so match `5.1.`
    // anywhere rather than only on a leading boundary.
    if category == Channels {
        return hay.contains(&format!("{token}."));
    }
    if hay.contains(&format!(".{token}.")) {
        return true;
    }
    // Audio codecs glue onto the channel count (`DDP5.1`, `AAC2.0`), so
    // also accept `<token><digit>`. Other categories must land on a `.`
    // boundary (so `hdr` ≠ `hdr10`, `web` ≠ `webrip`).
    if category != AudioCodec {
        return false;
    }
    let needle = format!(".{token}");
    match hay.find(&needle) {
        Some(start) => hay[start + needle.len()..]
            .chars()
            .next()
            .is_some_and(char::is_numeric),
        None => false,
    }
}

/// `(filename as NSString).deletingPathExtension`: drops the last
/// `.ext` of the last path component when the extension is non-empty and
/// the dot is not the component's first character.
fn deleting_path_extension(filename: &str) -> &str {
    let component_start = filename.rfind('/').map_or(0, |i| i + 1);
    let component = &filename[component_start..];
    match component.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < component.len() => &filename[..component_start + dot],
        _ => filename,
    }
}
