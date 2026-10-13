use std::fmt;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::serde_support::swift_double_opt;
use crate::source_url::SourceUrl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Progressive,
    VideoOnly,
    AudioOnly,
}

/// How a format's bytes are delivered. `Direct` is a single
/// `Range`-capable http(s) URL the engine downloads itself; `Hls` / `Dash`
/// are segmented manifests with no single URL, handed to yt-dlp wholesale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaDelivery {
    Direct,
    Hls,
    Dash,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Serialize, Deserialize)]
struct OtherBody {
    #[serde(rename = "_0")]
    value: String,
}

/// Implements Swift's synthesized `Codable` for an enum of payload-less
/// cases plus `other(String)`: `{"av1":{}}`, `{"other":{"_0":"x"}}`.
macro_rules! swift_tagged_serde {
    ($ty:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(1))?;
                match self {
                    $($ty::$variant => map.serialize_entry($name, &Empty {})?,)+
                    $ty::Other(value) => map.serialize_entry("other", &OtherBody {
                        value: value.clone(),
                    })?,
                }
                map.end()
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = $ty;

                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str(concat!("a ", stringify!($ty), " object with one case key"))
                    }

                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<$ty, A::Error> {
                        let key: String = map.next_key()?.ok_or_else(|| {
                            de::Error::custom(concat!(stringify!($ty), " object has no case key"))
                        })?;
                        let value = match key.as_str() {
                            $($name => {
                                let _: IgnoredAny = map.next_value()?;
                                $ty::$variant
                            })+
                            "other" => {
                                let body: OtherBody = map.next_value()?;
                                $ty::Other(body.value)
                            }
                            other => {
                                return Err(de::Error::custom(format!(
                                    concat!("Unknown ", stringify!($ty), " case `{}`"),
                                    other
                                )));
                            }
                        };
                        if map.next_key::<IgnoredAny>()?.is_some() {
                            return Err(de::Error::custom(concat!(
                                stringify!($ty),
                                " object has more than one case key"
                            )));
                        }
                        Ok(value)
                    }
                }
                deserializer.deserialize_map(V)
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    Av1,
    Vp9,
    H264,
    Other(String),
}

swift_tagged_serde!(VideoCodec { Av1 => "av1", Vp9 => "vp9", H264 => "h264" });

impl VideoCodec {
    pub const PRIORITY: [VideoCodec; 3] = [VideoCodec::Av1, VideoCodec::Vp9, VideoCodec::H264];

    /// Position in [`Self::PRIORITY`]; `usize::MAX` (Swift `Int.max`) for
    /// codecs not in it.
    pub fn rank(&self) -> usize {
        Self::PRIORITY
            .iter()
            .position(|c| c == self)
            .unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Opus,
    Aac,
    Other(String),
}

swift_tagged_serde!(AudioCodec { Opus => "opus", Aac => "aac" });

impl AudioCodec {
    pub const PRIORITY: [AudioCodec; 2] = [AudioCodec::Opus, AudioCodec::Aac];

    /// Position in [`Self::PRIORITY`]; `usize::MAX` for codecs not in it.
    pub fn rank(&self) -> usize {
        Self::PRIORITY
            .iter()
            .position(|c| c == self)
            .unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MediaContainer {
    Mp4,
    Webm,
    M4a,
    Other(String),
}

swift_tagged_serde!(MediaContainer { Mp4 => "mp4", Webm => "webm", M4a => "m4a" });

impl MediaContainer {
    pub const PRIORITY: [MediaContainer; 2] = [MediaContainer::Mp4, MediaContainer::Webm];

    /// Position in [`Self::PRIORITY`]; `usize::MAX` for containers not in it.
    pub fn rank(&self) -> usize {
        Self::PRIORITY
            .iter()
            .position(|c| c == self)
            .unwrap_or(usize::MAX)
    }

    pub fn file_extension(&self) -> &str {
        match self {
            MediaContainer::Mp4 => "mp4",
            MediaContainer::Webm => "webm",
            MediaContainer::M4a => "m4a",
            MediaContainer::Other(ext) => ext,
        }
    }

    pub fn from_file_extension(ext: &str) -> MediaContainer {
        match ext.to_lowercase().as_str() {
            "mp4" | "m4v" => MediaContainer::Mp4,
            "webm" => MediaContainer::Webm,
            "m4a" => MediaContainer::M4a,
            "" => MediaContainer::Other("bin".into()),
            lowered => MediaContainer::Other(lowered.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormat {
    pub id: String,
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcodec: Option<VideoCodec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acodec: Option<AudioCodec>,
    pub container: MediaContainer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesize: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesize_approx: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "swift_double_opt"
    )]
    pub tbr: Option<f64>,
    pub url: SourceUrl,
    pub delivery: MediaDelivery,
}

impl MediaFormat {
    /// A `Direct` format; use [`Self::with_delivery`] for HLS/DASH.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        kind: MediaKind,
        height: Option<i64>,
        width: Option<i64>,
        vcodec: Option<VideoCodec>,
        acodec: Option<AudioCodec>,
        container: MediaContainer,
        filesize: Option<i64>,
        filesize_approx: Option<i64>,
        tbr: Option<f64>,
        url: SourceUrl,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            height,
            width,
            vcodec,
            acodec,
            container,
            filesize,
            filesize_approx,
            tbr,
            url,
            delivery: MediaDelivery::Direct,
        }
    }

    pub fn with_delivery(mut self, delivery: MediaDelivery) -> Self {
        self.delivery = delivery;
        self
    }

    pub fn filesize_effective(&self) -> Option<i64> {
        self.filesize.or(self.filesize_approx)
    }

    pub fn is_approximate_size(&self) -> bool {
        self.filesize.is_none()
    }

    /// A single `Range`-capable URL the engine downloads itself.
    pub fn is_direct(&self) -> bool {
        self.delivery == MediaDelivery::Direct
    }
}
