use std::cmp::Ordering;

use sdm_core::{
    FormatChoice, MediaContainer, MediaFormat, MediaKind, QualityPreferences, ResolvedMedia,
};

/// Pure selection of the best format(s) for a resolved video given the
/// user's quality preferences. Fixture-tested, no I/O. Parent spec §4.3.
pub enum FormatSelector {}

/// A strict-weak "less than" as an `Ordering`, for a stable sort.
fn ordering(
    less: impl Fn(&MediaFormat, &MediaFormat) -> bool,
    a: &MediaFormat,
    b: &MediaFormat,
) -> Ordering {
    if less(a, b) {
        Ordering::Less
    } else if less(b, a) {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn has_video(format: &MediaFormat) -> bool {
    format.kind == MediaKind::VideoOnly || format.kind == MediaKind::Progressive
}

impl FormatSelector {
    pub fn pick(media: &ResolvedMedia, prefs: &QualityPreferences) -> Option<FormatChoice> {
        // Rank direct and HLS/DASH video formats together by quality, so a
        // 1080p HLS variant beats a metadata-less direct 360p (xnxx/xvideos)
        // while a real direct 1080p beats an equivalent HLS one.
        if let Some(best) = Self::eligible_video_formats(media, prefs).first()
            && let Some(choice) = Self::video_choice(best, media, prefs)
        {
            return Some(choice);
        }

        // Nothing fits the height ceiling. An HLS/DASH-only video still gets
        // its smallest variant downloaded wholesale (better than
        // "unsupported"); a direct video above the cap is left alone — the
        // user set the ceiling and a resumable download honors it.
        let smallest_streamed = media
            .formats
            .iter()
            .filter(|f| !f.is_direct() && has_video(f))
            .min_by_key(|f| f.height.unwrap_or(i64::MAX));
        if let Some(smallest) = smallest_streamed
            && let Some(choice) = Self::video_choice(smallest, media, prefs)
        {
            return Some(choice);
        }

        // Audio-only media — only when there is no video-bearing format at all.
        let has_any_video = media.formats.iter().any(has_video);
        if !has_any_video
            && let Some(audio) = Self::ranked_audio_formats(media, prefs).into_iter().next()
        {
            let container = audio.container.clone();
            let size = audio.filesize_effective();
            return Some(FormatChoice::new(None, Some(audio), container, size));
        }
        None
    }

    /// The yt-dlp `-f` expression a wholesale (HLS/DASH) download uses. Leans
    /// on yt-dlp's own selection rather than pinning one manifest variant.
    pub fn wholesale_selector(max_height: i64) -> String {
        format!("bv*[height<={max_height}]+ba/b[height<={max_height}]/bv*+ba/b")
    }

    /// A concrete choice for `best`, or `None` when it is a direct video-only
    /// stream with no eligible direct audio to mux against.
    fn video_choice(
        best: &MediaFormat,
        media: &ResolvedMedia,
        prefs: &QualityPreferences,
    ) -> Option<FormatChoice> {
        if !best.is_direct() {
            let container = if best.container == MediaContainer::Webm {
                MediaContainer::Webm
            } else {
                MediaContainer::Mp4
            };
            return Some(
                FormatChoice::new(
                    Some(best.clone()),
                    None,
                    container,
                    best.filesize_effective(),
                )
                .with_wholesale_selector(Self::wholesale_selector(prefs.max_height)),
            );
        }
        if best.kind == MediaKind::Progressive {
            return Some(FormatChoice::new(
                Some(best.clone()),
                None,
                best.container.clone(),
                best.filesize_effective(),
            ));
        }
        let audio = Self::ranked_audio_formats(media, prefs)
            .into_iter()
            .next()?;
        let size = Self::sum_size(best, &audio);
        Some(FormatChoice::new(
            Some(best.clone()),
            Some(audio),
            best.container.clone(),
            size,
        ))
    }

    /// Video-bearing formats (direct + HLS/DASH) that satisfy the codec /
    /// container allowlists and the max-height ceiling, best quality first.
    /// A `None` codec is unknown, not disqualifying: many extractors omit it.
    pub(crate) fn eligible_video_formats(
        media: &ResolvedMedia,
        prefs: &QualityPreferences,
    ) -> Vec<MediaFormat> {
        let mut formats: Vec<MediaFormat> = media
            .formats
            .iter()
            .filter(|format| {
                has_video(format)
                    && format.height.unwrap_or(0) <= prefs.max_height
                    && format
                        .vcodec
                        .as_ref()
                        .is_none_or(|c| prefs.video_codecs.contains(c))
                    && prefs.containers.contains(&format.container)
            })
            .cloned()
            .collect();
        formats.sort_by(|a, b| ordering(Self::video_rank_less, a, b));
        formats
    }

    pub fn ranked_video_formats(
        media: &ResolvedMedia,
        prefs: &QualityPreferences,
    ) -> Vec<MediaFormat> {
        Self::eligible_video_formats(media, prefs)
            .into_iter()
            .filter(|f| f.is_direct())
            .collect()
    }

    pub fn ranked_audio_formats(
        media: &ResolvedMedia,
        prefs: &QualityPreferences,
    ) -> Vec<MediaFormat> {
        let mut formats: Vec<MediaFormat> = media
            .formats
            .iter()
            .filter(|format| {
                format.is_direct()
                    && format.kind == MediaKind::AudioOnly
                    && format
                        .acodec
                        .as_ref()
                        .is_none_or(|c| prefs.audio_codecs.contains(c))
            })
            .cloned()
            .collect();
        formats.sort_by(|a, b| {
            ordering(
                |a, b| {
                    let ra = a.acodec.as_ref().map_or(usize::MAX, |c| c.rank());
                    let rb = b.acodec.as_ref().map_or(usize::MAX, |c| c.rank());
                    if ra != rb {
                        return ra < rb;
                    }
                    a.tbr.unwrap_or(0.0) > b.tbr.unwrap_or(0.0)
                },
                a,
                b,
            )
        });
        formats
    }

    /// Higher resolution first, then codec priority, then container
    /// priority, then higher bitrate. Shared with the picker UI so the
    /// auto-pick is always the first matching row in the list.
    pub fn video_rank_less(a: &MediaFormat, b: &MediaFormat) -> bool {
        let ha = a.height.unwrap_or(0);
        let hb = b.height.unwrap_or(0);
        if ha != hb {
            return ha > hb;
        }
        // At the same resolution a direct (resumable, no yt-dlp) stream wins
        // — even over a fancier-codec HLS/DASH one.
        if a.is_direct() != b.is_direct() {
            return a.is_direct();
        }
        let ca = a.vcodec.as_ref().map_or(usize::MAX, |c| c.rank());
        let cb = b.vcodec.as_ref().map_or(usize::MAX, |c| c.rank());
        if ca != cb {
            return ca < cb;
        }
        if a.container.rank() != b.container.rank() {
            return a.container.rank() < b.container.rank();
        }
        a.tbr.unwrap_or(0.0) > b.tbr.unwrap_or(0.0)
    }

    fn sum_size(a: &MediaFormat, b: &MediaFormat) -> Option<i64> {
        Some(a.filesize_effective()? + b.filesize_effective()?)
    }
}
