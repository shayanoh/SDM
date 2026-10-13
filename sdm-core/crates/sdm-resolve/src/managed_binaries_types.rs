use std::cmp::Ordering;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

/// yt-dlp release channel. Stable ships ~monthly; nightly publishes on any
/// day the codebase changes and is the safer choice against YouTube breakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum YtDlpChannel {
    Stable,
    Nightly,
}

impl YtDlpChannel {
    /// Swift `CaseIterable.allCases`.
    pub const ALL: [YtDlpChannel; 2] = [YtDlpChannel::Stable, YtDlpChannel::Nightly];

    fn repo(self) -> &'static str {
        match self {
            YtDlpChannel::Stable => "yt-dlp/yt-dlp",
            YtDlpChannel::Nightly => "yt-dlp/yt-dlp-nightly-builds",
        }
    }

    /// GitHub "latest release" API for this channel.
    pub fn releases_api(self) -> Url {
        static_url(&format!(
            "https://api.github.com/repos/{}/releases/latest",
            self.repo()
        ))
    }

    /// Direct download URL for the universal2 macOS binary of a given tag.
    pub fn asset_url(self, tag: &str) -> Url {
        static_url(&format!(
            "https://github.com/{}/releases/download/{tag}/yt-dlp_macos",
            self.repo()
        ))
    }

    /// The `SHA2-256SUMS` manifest for a given tag.
    pub fn sums_url(self, tag: &str) -> Url {
        static_url(&format!(
            "https://github.com/{}/releases/download/{tag}/SHA2-256SUMS",
            self.repo()
        ))
    }
}

/// The scheme and host are fixed, so parsing cannot fail whatever `tag`
/// holds (it only ever lands in the path, and `Url` percent-encodes it).
fn static_url(text: &str) -> Url {
    Url::parse(text).expect("fixed https URL with an encoded tag")
}

/// A yt-dlp version tag: `YYYY.MM.DD` (stable) or `YYYY.MM.DD.HHMMSS`
/// (nightly). Compared component-wise, shorter padded with zeros.
///
/// Equality is exact component equality (Swift's synthesized `==`), so
/// `2026.08.19` != `2026.08.19.0` although neither orders before the other,
/// exactly as in Swift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlpVersion {
    pub components: Vec<i64>,
}

impl YtDlpVersion {
    /// `None` unless every dot-separated part parses as an integer.
    pub fn new(tag: &str) -> Option<Self> {
        let trimmed = tag.trim();
        let mut parsed = Vec::new();
        for part in trimmed.split('.') {
            parsed.push(part.parse::<i64>().ok()?);
        }
        Some(Self { components: parsed })
    }
}

impl PartialOrd for YtDlpVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let count = self.components.len().max(other.components.len());
        for index in 0..count {
            let l = self.components.get(index).copied().unwrap_or(0);
            let r = other.components.get(index).copied().unwrap_or(0);
            if l != r {
                return Some(l.cmp(&r));
            }
        }
        Some(Ordering::Equal)
    }
}

impl fmt::Display for YtDlpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.components.iter().map(i64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

/// Persisted at `<binDir>/manifest.json`. Tracks what is installed so a
/// launch check knows whether to download.
///
/// Swift's synthesized `Codable`: camelCase keys, `nil` optionals omitted,
/// `ytDlpChannel` and `lastCheckTick` required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinariesManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yt_dlp_version: Option<String>,
    pub yt_dlp_channel: YtDlpChannel,
    pub last_check_tick: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qjs_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl BinariesManifest {
    pub fn empty() -> Self {
        Self {
            yt_dlp_version: None,
            yt_dlp_channel: YtDlpChannel::Stable,
            last_check_tick: 0,
            ffmpeg_version: None,
            qjs_version: None,
            last_error: None,
        }
    }

    pub fn version_for(&self, asset_name: &str) -> Option<&str> {
        match asset_name {
            "ffmpeg" | "ffprobe" => self.ffmpeg_version.as_deref(),
            "qjs" => self.qjs_version.as_deref(),
            _ => None,
        }
    }

    /// The bytes Swift's `JSONEncoder` writes with `.prettyPrinted` and
    /// `.sortedKeys` (what `ManagedBinaries.saveManifest` stores): keys
    /// sorted, two-space indent, `"key" : value` with a space before the
    /// colon, `/` escaped as `\/`, other control characters as lowercase
    /// `\u00xx`, non-ASCII raw, and no trailing newline. The manifest holds
    /// no `Date`s (Swift would write those as a `Double` of seconds since
    /// 2001), so none are handled here.
    pub fn to_foundation_json(&self) -> Vec<u8> {
        let mut fields: Vec<(&str, FoundationValue)> = Vec::new();
        if let Some(v) = &self.yt_dlp_version {
            fields.push(("ytDlpVersion", FoundationValue::Str(v)));
        }
        let channel = match self.yt_dlp_channel {
            YtDlpChannel::Stable => "stable",
            YtDlpChannel::Nightly => "nightly",
        };
        fields.push(("ytDlpChannel", FoundationValue::Str(channel)));
        fields.push(("lastCheckTick", FoundationValue::Int(self.last_check_tick)));
        if let Some(v) = &self.ffmpeg_version {
            fields.push(("ffmpegVersion", FoundationValue::Str(v)));
        }
        if let Some(v) = &self.qjs_version {
            fields.push(("qjsVersion", FoundationValue::Str(v)));
        }
        if let Some(v) = &self.last_error {
            fields.push(("lastError", FoundationValue::Str(v)));
        }
        fields.sort_by(|a, b| a.0.cmp(b.0));

        let mut out = String::from("{\n");
        for (index, (key, value)) in fields.iter().enumerate() {
            if index > 0 {
                out.push_str(",\n");
            }
            out.push_str("  ");
            write_escaped(&mut out, key);
            out.push_str(" : ");
            match value {
                FoundationValue::Str(s) => write_escaped(&mut out, s),
                FoundationValue::Int(n) => out.push_str(&n.to_string()),
            }
        }
        out.push_str("\n}");
        out.into_bytes()
    }
}

enum FoundationValue<'a> {
    Str(&'a str),
    Int(i64),
}

fn write_escaped(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A binary shipped compressed inside the `.app` bundle, to be inflated into
/// the managed `bin/` directory. `name` is `"ffmpeg"`, `"ffprobe"` or `"qjs"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorAsset {
    pub name: String,
    pub compressed_path: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckReason {
    Launch,
    Timer,
    ResolveNeeded,
    ChannelChanged,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    UpToDate(Option<String>),
    Updated(String),
    NoNetwork,
    Failed(String),
    Skipped,
}

/// Read by the Settings "Components" section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedBinariesStatus {
    pub yt_dlp_version: Option<String>,
    pub latest_known: Option<String>,
    pub channel: YtDlpChannel,
    pub ffmpeg_version: Option<String>,
    pub qjs_version: Option<String>,
    pub last_check_tick: Option<i64>,
    pub last_error: Option<String>,
}
