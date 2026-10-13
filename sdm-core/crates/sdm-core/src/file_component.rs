use std::fmt;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::range_set::RangeSet;
use crate::serde_support::uuid_upper;
use crate::source_url::SourceUrl;

/// Where a component's bytes come from.
///
/// `Resolved` marks a stream a `LinkResolver` produced: when its URL
/// expires mid-download the engine calls `resolver.refresh(source_url:
/// DownloadItem.source_url, format_id:)` to get a fresh one. Only the
/// `format_id` is stored — the video's identity lives once, in
/// `DownloadItem.source_url`. Parent spec §5.1.
///
/// Wire shape (Swift's custom `Codable`): `{"kind":"http"}`,
/// `{"kind":"resolved","formatID":"137"}`, `{"kind":"wholesale",
/// "formatID":"<selector>"}` (the selector is stored under `formatID`).
/// Reading also accepts the legacy synthesized `{"resolved":{...,
/// "formatID":"137"}}`; an unknown `kind`, `{"http":{}}` or anything else
/// decodes as `Http`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ComponentOrigin {
    Http,
    Resolved {
        format_id: String,
    },
    /// A stream with no single `Range`-capable URL (HLS/DASH). The engine
    /// hands `DownloadItem.source_url` to yt-dlp as a downloader with this
    /// `-f` selector; the component is always non-resumable. Parent spec
    /// `2026-09-03-multi-site-resolver-design.md` §6.4.
    Wholesale {
        format_selector: String,
    },
}

impl Serialize for ComponentOrigin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ComponentOrigin::Http => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("kind", "http")?;
                map.end()
            }
            ComponentOrigin::Resolved { format_id } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "resolved")?;
                map.serialize_entry("formatID", format_id)?;
                map.end()
            }
            ComponentOrigin::Wholesale { format_selector } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "wholesale")?;
                map.serialize_entry("formatID", format_selector)?;
                map.end()
            }
        }
    }
}

/// The legacy `resolved` value: any object is accepted (Swift's
/// `try? nestedContainer`), but then `formatID` is required.
#[derive(Deserialize)]
#[serde(untagged)]
enum LegacyResolved {
    Keyed {
        #[serde(rename = "formatID")]
        format_id: Option<String>,
    },
    NotAnObject(IgnoredAny),
}

struct OriginVisitor;

impl<'de> Visitor<'de> for OriginVisitor {
    type Value = ComponentOrigin;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a ComponentOrigin object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ComponentOrigin, A::Error> {
        let mut kind: Option<String> = None;
        let mut format_id: Option<String> = None;
        let mut legacy: Option<LegacyResolved> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "kind" => kind = map.next_value::<Option<String>>()?,
                "formatID" => format_id = map.next_value::<Option<String>>()?,
                "resolved" => legacy = Some(map.next_value()?),
                _ => {
                    let _: IgnoredAny = map.next_value()?;
                }
            }
        }
        // Current shape. An unrecognized `kind` (forward-compat) falls
        // through to the legacy check and then to `Http`.
        if let Some(kind) = kind.as_deref() {
            let need_format_id =
                |field: Option<String>| field.ok_or_else(|| de::Error::missing_field("formatID"));
            match kind {
                "http" => return Ok(ComponentOrigin::Http),
                "resolved" => {
                    return Ok(ComponentOrigin::Resolved {
                        format_id: need_format_id(format_id)?,
                    });
                }
                "wholesale" => {
                    return Ok(ComponentOrigin::Wholesale {
                        format_selector: need_format_id(format_id)?,
                    });
                }
                _ => {}
            }
        }
        // Legacy synthesized shape: {"resolved":{"extractor":…,"videoID":…,
        // "formatID":"137"}}; anything else (incl. {"http":{}}) is `Http`.
        if let Some(LegacyResolved::Keyed { format_id }) = legacy {
            return Ok(ComponentOrigin::Resolved {
                format_id: format_id.ok_or_else(|| de::Error::missing_field("formatID"))?,
            });
        }
        Ok(ComponentOrigin::Http)
    }
}

impl<'de> Deserialize<'de> for ComponentOrigin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(OriginVisitor)
    }
}

/// What has to happen once every component of an item is fully downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Assembly {
    /// One component whose container is already final — just rename it.
    None,
    /// Combine the components with `ffmpeg -c copy`.
    Mux,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentError {
    pub message: String,
}

impl ComponentError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One separately-downloaded file backing a `DownloadItem`. A generic HTTP
/// download has exactly one; a muxed YouTube download has two (video +
/// audio). Parent spec §5.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileComponent {
    #[serde(with = "uuid_upper")]
    pub id: Uuid,
    pub url: SourceUrl,
    pub part_filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<i64>,
    pub completed: RangeSet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<String>,
    pub origin: ComponentOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_resumable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<ComponentError>,
}

impl FileComponent {
    /// A component with a fresh id, no known size, nothing completed and
    /// `Http` origin. The other Swift initializer arguments are the
    /// `with_*` builders (or plain field assignment).
    pub fn new(url: SourceUrl, part_filename: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            url,
            part_filename: part_filename.into(),
            total_bytes: None,
            completed: RangeSet::new(),
            validator: None,
            origin: ComponentOrigin::Http,
            is_resumable: None,
            last_error: None,
        }
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_total_bytes(mut self, total_bytes: i64) -> Self {
        self.total_bytes = Some(total_bytes);
        self
    }

    pub fn with_completed(mut self, completed: RangeSet) -> Self {
        self.completed = completed;
        self
    }

    pub fn with_validator(mut self, validator: impl Into<String>) -> Self {
        self.validator = Some(validator.into());
        self
    }

    pub fn with_origin(mut self, origin: ComponentOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_is_resumable(mut self, is_resumable: Option<bool>) -> Self {
        self.is_resumable = is_resumable;
        self
    }

    pub fn with_last_error(mut self, last_error: ComponentError) -> Self {
        self.last_error = Some(last_error);
        self
    }

    pub fn is_complete(&self) -> bool {
        match self.total_bytes {
            Some(total) => self.completed.is_complete(total),
            None => false,
        }
    }

    pub fn fraction_completed(&self) -> f64 {
        match self.total_bytes {
            Some(total) if total > 0 => self.completed.total_bytes() as f64 / total as f64,
            _ => 0.0,
        }
    }
}
