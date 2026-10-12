use sdm_core::{WholesalePhase, WholesaleProgress};

const POST_PROCESS_MARKERS: [&str; 7] = [
    "[merger]",
    "merging formats",
    "[extractaudio]",
    "[fixupm3u8]",
    "[videoconvertor]",
    "[fixup",
    "[videoremuxer]",
];

/// Parses one line of `yt-dlp` output (as a wholesale downloader) into a
/// `WholesaleProgress`, or `None` for lines that carry no progress signal.
///
/// Download lines come from
/// `--progress-template "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.fragment_index)s|%(progress.fragment_count)s"`.
///
/// For HLS/DASH downloads `total_bytes` is `NA`, `total_bytes_estimate` is a
/// float string, and the reliable progress signal is
/// `fragment_index / fragment_count`. Post-processing (merge / fixup /
/// extract-audio) is detected by yt-dlp's bracketed prefixes. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.6.
pub enum WholesaleProgressParser {}

/// Foundation's `CharacterSet.whitespaces`: general category Zs plus tab
/// (not newlines, unlike `str::trim`).
fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

fn trim(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

impl WholesaleProgressParser {
    pub fn parse(line: &str) -> Option<WholesaleProgress> {
        let trimmed = trim(line);
        if let Some(body) = trimmed.strip_prefix("sdm:") {
            return Self::parse_download_line(body);
        }
        let lower = trimmed.to_lowercase();
        if POST_PROCESS_MARKERS.iter().any(|m| lower.contains(m)) {
            return Some(WholesaleProgress {
                phase: WholesalePhase::PostProcessing,
                ..WholesaleProgress::default()
            });
        }
        None
    }

    fn parse_download_line(body: &str) -> Option<WholesaleProgress> {
        let fields: Vec<&str> = body.split('|').map(trim).collect();
        if fields.len() < 4 {
            return None;
        }

        let downloaded = Self::number(fields[1]);
        let total = Self::number(fields[2]);
        let estimate = Self::number(fields[3]);

        let mut fraction = None;
        let mut is_fragmented = false;
        if fields.len() >= 6
            && let (Some(index), Some(count)) = (Self::number(fields[4]), Self::number(fields[5]))
            && count > 0
        {
            fraction = Some((index as f64 / count as f64).clamp(0.0, 1.0));
            is_fragmented = true;
        }

        Some(WholesaleProgress {
            downloaded_bytes: downloaded,
            total_bytes: total.or(estimate),
            fraction,
            phase: WholesalePhase::Downloading,
            is_fragmented,
        })
    }

    /// yt-dlp numbers arrive as integers, comma-grouped, or — for
    /// `total_bytes_estimate` on fragmented downloads — floats (`"1.3e10"` /
    /// `"13161769424.0"`). `NA` / `None` / empty → `None`.
    fn number(raw: &str) -> Option<i64> {
        let cleaned = raw.replace(',', "");
        if cleaned.is_empty() || cleaned.to_uppercase() == "NA" || cleaned == "None" {
            return None;
        }
        if let Ok(value) = cleaned.parse::<i64>()
            && value >= 0
        {
            return Some(value);
        }
        if let Ok(value) = cleaned.parse::<f64>()
            && value.is_finite()
            && value >= 0.0
        {
            // Saturates where Swift's `Int64(_:)` would trap.
            return Some(value.round() as i64);
        }
        None
    }
}
