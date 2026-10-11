use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::media_format::{AudioCodec, MediaContainer, MediaFormat, VideoCodec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityPreferences {
    pub max_height: i64,
    pub video_codecs: HashSet<VideoCodec>,
    pub containers: HashSet<MediaContainer>,
    pub audio_codecs: HashSet<AudioCodec>,
}

impl Default for QualityPreferences {
    /// Swift `QualityPreferences.default`: up to 1080p, every known codec
    /// and container.
    fn default() -> Self {
        Self {
            max_height: 1080,
            video_codecs: HashSet::from([VideoCodec::Av1, VideoCodec::Vp9, VideoCodec::H264]),
            containers: HashSet::from([MediaContainer::Mp4, MediaContainer::Webm]),
            audio_codecs: HashSet::from([AudioCodec::Opus, AudioCodec::Aac]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<MediaFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<MediaFormat>,
    pub output_container: MediaContainer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<i64>,
    /// Some ⇒ this choice is a wholesale (yt-dlp-as-downloader) job for
    /// an HLS/DASH-only stream. The string is a yt-dlp `-f` selector.
    /// `video`/`audio` stay populated for the picker label; the engine
    /// ignores them for a wholesale component. Parent spec §6.2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wholesale_selector: Option<String>,
}

impl FormatChoice {
    pub fn new(
        video: Option<MediaFormat>,
        audio: Option<MediaFormat>,
        output_container: MediaContainer,
        estimated_bytes: Option<i64>,
    ) -> Self {
        Self {
            video,
            audio,
            output_container,
            estimated_bytes,
            wholesale_selector: None,
        }
    }

    pub fn with_wholesale_selector(mut self, selector: impl Into<String>) -> Self {
        self.wholesale_selector = Some(selector.into());
        self
    }

    pub fn is_wholesale(&self) -> bool {
        self.wholesale_selector.is_some()
    }

    pub fn requires_mux(&self) -> bool {
        self.video.is_some() && self.audio.is_some() && self.wholesale_selector.is_none()
    }

    pub fn format_ids(&self) -> Vec<String> {
        [&self.video, &self.audio]
            .into_iter()
            .flatten()
            .map(|f| f.id.clone())
            .collect()
    }
}
