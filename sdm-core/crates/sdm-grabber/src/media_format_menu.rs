use std::cmp::Ordering;

use sdm_core::{
    FormatChoice, MediaContainer, MediaFormat, MediaKind, QualityPreferences, ResolvedMedia,
};
use sdm_resolve::FormatSelector;

use crate::media_metadata::describe_video;

/// One selectable entry in a media row's format picker. Parent spec §9.2.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaFormatOption {
    pub id: String,
    pub label: String,
    pub choice: FormatChoice,
    pub matches_preferences: bool,
    /// This option is an HLS/DASH stream that yt-dlp downloads wholesale
    /// (non-resumable). The picker badges it "streamed".
    pub is_wholesale: bool,
}

/// Pure builder of the flat, match-first format-picker list. Matching
/// options (pass every allowlist + max-res filter) come first, then
/// non-matching; each group is ranked by `FormatSelector::video_rank_less`,
/// so the auto-pick is always the first matching entry.
pub enum MediaFormatMenu {}

impl MediaFormatMenu {
    pub fn options(
        media: &ResolvedMedia,
        preferences: &QualityPreferences,
    ) -> Vec<MediaFormatOption> {
        let best_eligible_audio = FormatSelector::ranked_audio_formats(media, preferences)
            .into_iter()
            .next();
        // `sorted { tbr > tbr }.first`: the highest bitrate, earliest on ties.
        let mut best_any_audio: Option<&MediaFormat> = None;
        for format in media
            .formats
            .iter()
            .filter(|f| f.kind == MediaKind::AudioOnly && f.is_direct())
        {
            if best_any_audio.is_none_or(|best| format.tbr.unwrap_or(0.0) > best.tbr.unwrap_or(0.0))
            {
                best_any_audio = Some(format);
            }
        }

        let mut options: Vec<MediaFormatOption> = Vec::new();

        // Only `.direct` formats get a plain-download row. HLS/DASH
        // progressive / video-only formats are handled by the `streamed`
        // section below — a "normal" row for one would point the engine at
        // an `.m3u8` playlist and save it verbatim as `.mp4`.
        for video in media
            .formats
            .iter()
            .filter(|f| f.kind == MediaKind::Progressive && f.is_direct())
        {
            let matches = video_matches(video, preferences);
            options.push(option(video, None, matches));
        }

        for video in media
            .formats
            .iter()
            .filter(|f| f.kind == MediaKind::VideoOnly && f.is_direct())
        {
            let video_matches = video_matches(video, preferences);
            let audio = best_eligible_audio.as_ref().or(best_any_audio);
            let Some(audio) = audio else { continue };
            let matches = video_matches && best_eligible_audio.is_some();
            options.push(option(video, Some(audio), matches));
        }

        options.sort_by(|lhs, rhs| {
            if lhs.matches_preferences != rhs.matches_preferences {
                return if lhs.matches_preferences {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            let (Some(lv), Some(rv)) = (lhs.choice.video.as_ref(), rhs.choice.video.as_ref())
            else {
                return Ordering::Equal;
            };
            if FormatSelector::video_rank_less(lv, rv) {
                Ordering::Less
            } else if FormatSelector::video_rank_less(rv, lv) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });

        // HLS/DASH variants land after every direct option, tallest first.
        let mut streamed: Vec<&MediaFormat> = media
            .formats
            .iter()
            .filter(|f| {
                !f.is_direct() && matches!(f.kind, MediaKind::VideoOnly | MediaKind::Progressive)
            })
            .collect();
        streamed.sort_by_key(|f| std::cmp::Reverse(f.height.unwrap_or(0)));
        options.extend(streamed.into_iter().map(wholesale_option));
        options
    }
}

fn video_matches(video: &MediaFormat, preferences: &QualityPreferences) -> bool {
    video.height.unwrap_or(0) <= preferences.max_height
        && video
            .vcodec
            .as_ref()
            .is_none_or(|c| preferences.video_codecs.contains(c))
        && preferences.containers.contains(&video.container)
}

fn wholesale_option(format: &MediaFormat) -> MediaFormatOption {
    let selector = if format.kind == MediaKind::Progressive {
        format.id.clone()
    } else {
        format!("{}+ba/b", format.id)
    };
    let height = format
        .height
        .map_or_else(|| "auto".to_owned(), |h| format!("{h}p"));
    MediaFormatOption {
        id: format!("w:{}", format.id),
        label: format!("{height} · streamed"),
        choice: FormatChoice::new(
            Some(format.clone()),
            None,
            MediaContainer::Mp4,
            format.filesize_effective(),
        )
        .with_wholesale_selector(selector),
        matches_preferences: false,
        is_wholesale: true,
    }
}

fn option(video: &MediaFormat, audio: Option<&MediaFormat>, matches: bool) -> MediaFormatOption {
    let container = video.container.clone();
    let estimated = combined_size(video, audio);
    MediaFormatOption {
        id: format!("{}+{}", video.id, audio.map_or("-", |a| a.id.as_str())),
        label: label(video, audio, &container, estimated),
        choice: FormatChoice::new(
            Some(video.clone()),
            if video.kind == MediaKind::Progressive {
                None
            } else {
                audio.cloned()
            },
            container,
            estimated,
        ),
        matches_preferences: matches,
        is_wholesale: false,
    }
}

fn combined_size(video: &MediaFormat, audio: Option<&MediaFormat>) -> Option<i64> {
    if video.kind == MediaKind::Progressive {
        return video.filesize_effective();
    }
    Some(video.filesize_effective()? + audio?.filesize_effective()?)
}

fn label(
    video: &MediaFormat,
    audio: Option<&MediaFormat>,
    container: &MediaContainer,
    estimated: Option<i64>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    match video.height {
        Some(height) => parts.push(format!("{height}p")),
        // No resolution metadata (e.g. xnxx `low` / `high`) — show the
        // format id so the row is still identifiable.
        None => parts.push(video.id.clone()),
    }
    if let Some(vcodec) = &video.vcodec {
        parts.push(describe_video(vcodec));
    }
    parts.push(container.file_extension().to_owned());
    let approximate = video.is_approximate_size() || audio.is_some_and(|a| a.is_approximate_size());
    match estimated {
        Some(bytes) => parts.push(format!(
            "{}{}",
            if approximate { "~" } else { "" },
            byte_count_string(bytes)
        )),
        None => parts.push("—".into()),
    }
    parts.join(" · ")
}

/// `ByteCountFormatter` with `.binary` count style (en_US conventions):
/// 1024-based steps, `Zero KB` for 0, `1 byte` / `N bytes` below 1 KB, then
/// KB with 0 fraction digits, MB with 1, GB and above with 2 (trailing zeros
/// dropped), rounding half away from zero, a `,` grouping separator on the
/// integer part. A value that rounds up to 1024 of its unit is shown in the
/// next unit up ("1 MB" for 1048575 bytes). Measured against Foundation.
fn byte_count_string(bytes: i64) -> String {
    if bytes == 0 {
        return "Zero KB".into();
    }
    let sign = if bytes < 0 { "-" } else { "" };
    let magnitude = bytes.unsigned_abs();
    if magnitude == 1 {
        return format!("{sign}1 byte");
    }
    if magnitude < 1024 {
        return format!("{sign}{} bytes", group_thousands(magnitude));
    }
    // (unit name, fraction digits)
    let units = [
        ("KB", 0),
        ("MB", 1),
        ("GB", 2),
        ("TB", 2),
        ("PB", 2),
        ("EB", 2),
    ];
    let mut value = magnitude as f64 / 1024.0;
    let mut unit = 0;
    loop {
        let scale = 10f64.powi(units[unit].1);
        let rounded = (value * scale).round() / scale;
        if rounded >= 1024.0 && unit + 1 < units.len() {
            value /= 1024.0;
            unit += 1;
            continue;
        }
        let digits = units[unit].1 as usize;
        let text = format!("{rounded:.digits$}");
        let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
        let whole = group_thousands(whole.parse().unwrap_or(0));
        let fraction = fraction.trim_end_matches('0');
        let number = if fraction.is_empty() {
            whole
        } else {
            format!("{whole}.{fraction}")
        };
        return format!("{sign}{number} {}", units[unit].0);
    }
}

/// `1234567` -> `"1,234,567"`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::byte_count_string;

    /// Exact `ByteCountFormatter(countStyle: .binary)` outputs measured on
    /// macOS with an en_US locale (decimal point ".", grouping separator ",").
    #[test]
    fn byte_count_string_matches_byte_count_formatter() {
        let table: [(i64, &str); 40] = [
            (0, "Zero KB"),
            (1, "1 byte"),
            (2, "2 bytes"),
            (999, "999 bytes"),
            (1000, "1,000 bytes"),
            (1023, "1,023 bytes"),
            (1024, "1 KB"),
            (1025, "1 KB"),
            (1536, "2 KB"),
            (2560, "3 KB"),
            (1_047_552, "1,023 KB"),
            (1_048_063, "1,023 KB"),
            (1_048_064, "1 MB"),
            (1_048_575, "1 MB"),
            (1_048_576, "1 MB"),
            (1_310_720, "1.3 MB"),
            (1_500_000, "1.4 MB"),
            (5_452_595, "5.2 MB"),
            (10_000_000, "9.5 MB"),
            (123_456_789, "117.7 MB"),
            (1_072_693_248, "1,023 MB"),
            (1_073_293_248, "1,023.6 MB"),
            (1_073_678_909, "1,023.9 MB"),
            (1_048_575_999, "1,000 MB"),
            (1_073_699_880, "1 GB"),
            (1_073_741_823, "1 GB"),
            (1_073_741_824, "1 GB"),
            (1_460_000_000, "1.36 GB"),
            (13_250_000_000, "12.34 GB"),
            (15_000_000_000, "13.97 GB"),
            (107_374_182_400, "100 GB"),
            (130_000_000_000, "121.07 GB"),
            (150_000_000_000, "139.7 GB"),
            (250_000_000_000, "232.83 GB"),
            (1_099_506_259_066, "1,023.99 GB"),
            (1_099_511_627_775, "1 TB"),
            (1_500_000_000_000, "1.36 TB"),
            (1_125_899_906_842_624, "1 PB"),
            (1_152_921_504_606_846_976, "1 EB"),
            (2_689_723_269, "2.5 GB"),
        ];
        for (bytes, expected) in table {
            assert_eq!(byte_count_string(bytes), expected, "{bytes} bytes");
        }
    }
}
