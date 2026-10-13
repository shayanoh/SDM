use serde::{Deserialize, Serialize};

use crate::media_format::MediaFormat;
use crate::serde_support::swift_double_opt;
use crate::source_url::SourceUrl;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedMedia {
    pub extractor: String,
    #[serde(rename = "videoID")]
    pub video_id: String,
    pub title: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "swift_double_opt"
    )]
    pub duration_seconds: Option<f64>,
    pub formats: Vec<MediaFormat>,
    /// The page URL this entry resolves from. Set for not-yet-resolved
    /// playlist entries (so the grabber re-resolves each against its own
    /// URL rather than a synthesized one); `None` for a directly resolved
    /// single. Parent spec `2026-09-03-multi-site-resolver-design.md` §5.3.
    #[serde(rename = "sourceURL", default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<SourceUrl>,
}

impl ResolvedMedia {
    pub fn new(
        extractor: impl Into<String>,
        video_id: impl Into<String>,
        title: impl Into<String>,
        duration_seconds: Option<f64>,
        formats: Vec<MediaFormat>,
    ) -> Self {
        Self {
            extractor: extractor.into(),
            video_id: video_id.into(),
            title: title.into(),
            duration_seconds,
            formats,
            source_url: None,
        }
    }

    pub fn with_source_url(mut self, source_url: SourceUrl) -> Self {
        self.source_url = Some(source_url);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedTarget {
    Single(ResolvedMedia),
    Playlist {
        title: String,
        entries: Vec<ResolvedMedia>,
        total_available: i64,
    },
}
