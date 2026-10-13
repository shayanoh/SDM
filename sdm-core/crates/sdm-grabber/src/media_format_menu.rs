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

/// `ByteCountFormatter` with `.binary` count style: 1024-based steps with
/// the usual `KB`/`MB`/`GB` unit names, whole numbers up to KB and one
/// fraction digit (dropped when zero) above, `Zero KB` for 0.
fn byte_count_string(bytes: i64) -> String {
    if bytes == 0 {
        return "Zero KB".into();
    }
    if bytes == 1 {
        return "1 byte".into();
    }
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let units = ["KB", "MB", "GB", "TB", "PB", "EB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{} KB", value.round() as i64);
    }
    let text = format!("{value:.1}");
    let text = text.strip_suffix(".0").unwrap_or(&text);
    format!("{text} {}", units[unit])
}
