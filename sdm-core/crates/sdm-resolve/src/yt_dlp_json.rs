use percent_encoding::percent_decode_str;
use sdm_core::{
    AudioCodec, MediaContainer, MediaDelivery, MediaFormat, MediaKind, ResolveError, ResolvedMedia,
    SourceUrl, VideoCodec,
};
use serde::{Deserialize, Deserializer};
use url::Url;

/// Decodes an optional integer the way Foundation's `JSONDecoder` decodes
/// `Int?` / `Int64?`: missing and `null` are `None`, a float is accepted
/// only when it is exactly an integer (`1080.0`, `2e3`), and anything else
/// (a fractional number, a string, a bool, an out-of-range value) fails the
/// whole decode.
fn int_opt<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    use serde::de::Error;
    let Some(number) = Option::<serde_json::Number>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if let Some(value) = number.as_i64() {
        return Ok(Some(value));
    }
    if number.as_u64().is_some() {
        return Err(D::Error::custom(format!(
            "Parsed JSON number <{number}> does not fit in Int."
        )));
    }
    match number.as_f64() {
        // 2^63 is exactly representable; every f64 below it converts exactly.
        Some(f)
            if f.is_finite()
                && f.fract() == 0.0
                && (-(2f64.powi(63))..2f64.powi(63)).contains(&f) =>
        {
            Ok(Some(f as i64))
        }
        _ => Err(D::Error::custom(format!(
            "Parsed JSON number <{number}> does not fit in Int."
        ))),
    }
}

/// Unknown keys are ignored and every field is optional, as with the Swift
/// `Decodable`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct YtDlpDump {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub extractor: Option<String>,
    #[serde(rename = "_type", default)]
    pub type_: Option<String>,
    #[serde(default)]
    pub formats: Option<Vec<YtDlpFormat>>,
    #[serde(default)]
    pub entries: Option<Vec<YtDlpEntry>>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct YtDlpFormat {
    #[serde(rename = "format_id", default)]
    pub format_id: Option<String>,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub vcodec: Option<String>,
    #[serde(default)]
    pub acodec: Option<String>,
    #[serde(default, deserialize_with = "int_opt")]
    pub height: Option<i64>,
    #[serde(default, deserialize_with = "int_opt")]
    pub width: Option<i64>,
    #[serde(default, deserialize_with = "int_opt")]
    pub filesize: Option<i64>,
    #[serde(rename = "filesize_approx", default, deserialize_with = "int_opt")]
    pub filesize_approx: Option<i64>,
    #[serde(default)]
    pub tbr: Option<f64>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "protocol", default)]
    pub proto: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct YtDlpEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "webpage_url", default)]
    pub webpage_url: Option<String>,
    #[serde(rename = "ie_key", default)]
    pub ie_key: Option<String>,
}

impl YtDlpDump {
    /// Decodes a `-J` dump. `serde`'s derived struct decoding would also
    /// accept a JSON array (positionally), which `JSONDecoder` rejects for a
    /// keyed container, so a top-level non-object is refused here first.
    /// (Nested elements keep serde's positional leniency; yt-dlp never emits
    /// those.)
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        use serde::de::Error;
        let first = bytes
            .iter()
            .find(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        if first != Some(&b'{') {
            return Err(serde_json::Error::custom(
                "Expected to decode a dictionary at the top level.",
            ));
        }
        serde_json::from_slice(bytes)
    }
}

pub enum YtDlpParser {}

/// Swift `URL.lastPathComponent`: the percent-decoded last non-empty path
/// segment, `/` for a root path.
fn last_path_component(url: &Url) -> String {
    // Decoded first, then split, as Foundation does (see the grabber's twin).
    let path = percent_decode_str(url.path())
        .decode_utf8_lossy()
        .into_owned();
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() {
            String::new()
        } else {
            "/".into()
        };
    }
    trimmed.rsplit('/').next().unwrap_or("").to_owned()
}

fn lower_in(ext: Option<&str>, list: &[&str]) -> bool {
    let lower = ext.unwrap_or("").to_lowercase();
    list.contains(&lower.as_str())
}

impl YtDlpParser {
    pub fn codec_for_vcodec(raw: Option<&str>) -> Option<VideoCodec> {
        let raw = raw?;
        if raw.is_empty() || raw == "none" {
            return None;
        }
        let lower = raw.to_lowercase();
        if lower.starts_with("av01") || lower.starts_with("av1") {
            return Some(VideoCodec::Av1);
        }
        if lower.starts_with("vp9") || lower.starts_with("vp09") {
            return Some(VideoCodec::Vp9);
        }
        if lower.starts_with("avc1") || lower.starts_with("h264") {
            return Some(VideoCodec::H264);
        }
        Some(VideoCodec::Other(raw.to_owned()))
    }

    pub fn codec_for_acodec(raw: Option<&str>) -> Option<AudioCodec> {
        let raw = raw?;
        if raw.is_empty() || raw == "none" {
            return None;
        }
        let lower = raw.to_lowercase();
        if lower.starts_with("opus") {
            return Some(AudioCodec::Opus);
        }
        if lower.starts_with("mp4a") || lower.starts_with("aac") {
            return Some(AudioCodec::Aac);
        }
        Some(AudioCodec::Other(raw.to_owned()))
    }

    /// Unlike `MediaContainer::from_file_extension`, an unrecognized
    /// extension keeps its original case.
    pub fn container(ext: Option<&str>) -> MediaContainer {
        match ext.unwrap_or("").to_lowercase().as_str() {
            "mp4" | "m4v" => MediaContainer::Mp4,
            "webm" => MediaContainer::Webm,
            "m4a" => MediaContainer::M4a,
            "" => MediaContainer::Other("bin".into()),
            _ => MediaContainer::Other(ext.unwrap_or("").to_owned()),
        }
    }

    pub fn is_video_container(ext: Option<&str>) -> bool {
        lower_in(
            ext,
            &[
                "mp4", "m4v", "webm", "mkv", "mov", "flv", "ts", "avi", "3gp",
            ],
        )
    }

    pub fn is_audio_container(ext: Option<&str>) -> bool {
        lower_in(
            ext,
            &["m4a", "mp3", "aac", "opus", "ogg", "oga", "flac", "wav"],
        )
    }

    /// yt-dlp `protocol` → how the engine must fetch it. `None` ⇒ this
    /// format is not usable (rtmp, ism, mss, …). Parent spec §6.1.
    pub fn delivery_for(proto: Option<&str>) -> Option<MediaDelivery> {
        match proto.unwrap_or("").to_lowercase().as_str() {
            "" | "https" | "http" | "https_native" => Some(MediaDelivery::Direct),
            "m3u8" | "m3u8_native" => Some(MediaDelivery::Hls),
            "http_dash_segments" => Some(MediaDelivery::Dash),
            _ => None,
        }
    }

    pub fn media_format(f: &YtDlpFormat) -> Option<MediaFormat> {
        let id = f.format_id.as_ref()?;
        let url_string = f.url.as_ref().filter(|s| !s.is_empty())?;
        // Lenient like Swift's `URL(string:)`: the string is kept as given
        // even when the `url` crate cannot parse it (see `SourceUrl`).
        let url: SourceUrl =
            serde_json::from_value(serde_json::Value::String(url_string.clone())).ok()?;
        if f.ext.as_deref().unwrap_or("") == "mhtml" {
            return None;
        }
        let delivery = Self::delivery_for(f.proto.as_deref())?;

        let vcodec = Self::codec_for_vcodec(f.vcodec.as_deref());
        let acodec = Self::codec_for_acodec(f.acodec.as_deref());
        let kind = match (&vcodec, &acodec) {
            (Some(_), Some(_)) => MediaKind::Progressive,
            (Some(_), None) => MediaKind::VideoOnly,
            (None, Some(_)) => MediaKind::AudioOnly,
            (None, None) => {
                // Some extractors (xnxx, xvideos, …) omit codec info even for
                // fully downloadable muxed streams. Infer the kind rather than
                // dropping the format: a real resolution ⇒ a video variant; a
                // direct single-file video container ⇒ a muxed stream. A
                // codec-less HLS entry with no resolution is ambiguous (often a
                // bare audio manifest, e.g. YouTube 233/234) — skip it.
                if f.height.is_some()
                    || (delivery == MediaDelivery::Direct
                        && Self::is_video_container(f.ext.as_deref()))
                {
                    MediaKind::Progressive
                } else if Self::is_audio_container(f.ext.as_deref())
                    || id.to_lowercase().contains("audio")
                {
                    MediaKind::AudioOnly
                } else {
                    return None;
                }
            }
        };

        Some(
            MediaFormat::new(
                id.clone(),
                kind,
                f.height,
                f.width,
                vcodec,
                acodec,
                Self::container(f.ext.as_deref()),
                f.filesize,
                f.filesize_approx,
                f.tbr,
                url,
            )
            .with_delivery(delivery),
        )
    }

    pub fn resolved_media(dump: &YtDlpDump) -> Result<ResolvedMedia, ResolveError> {
        let (Some(id), Some(raw_formats)) = (&dump.id, &dump.formats) else {
            return Err(ResolveError::Unavailable);
        };
        if raw_formats.is_empty() {
            return Err(ResolveError::Unavailable);
        }
        let formats: Vec<MediaFormat> = raw_formats.iter().filter_map(Self::media_format).collect();
        if formats.is_empty() {
            return Err(ResolveError::Unsupported);
        }
        Ok(ResolvedMedia::new(
            dump.extractor.clone().unwrap_or_else(|| "unknown".into()),
            id.clone(),
            dump.title.clone().unwrap_or_else(|| id.clone()),
            dump.duration,
            formats,
        ))
    }

    /// Playlist / channel entries with their own page URL (`(source_url,
    /// video_id, title)`). `entry.url` (the `--flat-playlist` field) is
    /// preferred, then `webpage_url` (present in a full `-J` playlist dump);
    /// relative values are resolved against the playlist URL. Entries with
    /// no usable URL are dropped. Parent spec §5.3.
    pub fn flat_entries(dump: &YtDlpDump, relative_to: &Url) -> Vec<(SourceUrl, String, String)> {
        dump.entries
            .iter()
            .flatten()
            .filter_map(|entry| {
                let raw = entry
                    .url
                    .as_deref()
                    .or(entry.webpage_url.as_deref())
                    .filter(|s| !s.is_empty())?;
                let resolved = relative_to.join(raw).ok()?;
                let id = entry
                    .id
                    .clone()
                    .unwrap_or_else(|| last_path_component(&resolved));
                let title = entry.title.clone().unwrap_or_else(|| id.clone());
                // An absolute value is kept as yt-dlp wrote it: the entry's
                // page URL is persisted, and Swift's `absoluteString` of an
                // absolute reference is that string.
                let source_url =
                    SourceUrl::new(raw).unwrap_or_else(|_| SourceUrl::from_url(&resolved));
                Some((source_url, id, title))
            })
            .collect()
    }
}
