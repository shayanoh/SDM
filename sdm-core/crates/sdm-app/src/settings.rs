//! The app's own settings, in `settings.json` inside the data directory. The
//! SwiftUI app keeps its settings in `UserDefaults`; the two apps share only
//! the download state.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use sdm_core::{AudioCodec, MediaContainer, QualityPreferences, VideoCodec};
use sdm_resolve::{CookieSource, YtDlpChannel};

use crate::error::AppError;
use crate::protocol::SettingsDto;
pub use crate::protocol::{ActivationMode, AutoClearInterval};

/// The resolutions the Media Sites popup offers, highest first.
pub const RESOLUTION_LADDER: [u32; 6] = [1920, 1440, 1080, 720, 540, 480];

const COOKIE_SOURCES: [&str; 6] = ["none", "safari", "chrome", "firefox", "edge", "brave"];

/// The live settings, shared with the engine, grabber and resolver closures.
/// Written only by `SettingsStore::update`.
pub type SharedSettings = Arc<RwLock<SettingsDto>>;

fn clamp(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

impl SettingsDto {
    /// A copy with every value forced into its legal range: what a loaded
    /// file becomes, so a hand-edited file never fails to load.
    pub fn clamped(mut self) -> Self {
        self.max_concurrent = clamp(self.max_concurrent, 1, 20);
        self.segments_per_item = clamp(self.segments_per_item, 1, 64);
        self.global_max_connections = clamp(self.global_max_connections, 1, 256);
        self.max_connections_per_host = clamp(self.max_connections_per_host, 1, 64);
        self.min_segment_size_mb = clamp(self.min_segment_size_mb, 1, 100);
        self.max_playlist_videos = clamp(self.max_playlist_videos, 10, 200);
        self.yt_max_height = RESOLUTION_LADDER
            .iter()
            .copied()
            .find(|rung| *rung <= self.yt_max_height)
            .unwrap_or(RESOLUTION_LADDER[RESOLUTION_LADDER.len() - 1]);
        if !(self.allow_av1 || self.allow_vp9 || self.allow_h264) {
            self.allow_av1 = true;
            self.allow_vp9 = true;
            self.allow_h264 = true;
        }
        if !(self.allow_mp4 || self.allow_webm) {
            self.allow_mp4 = true;
            self.allow_webm = true;
        }
        if !(self.allow_opus || self.allow_aac) {
            self.allow_opus = true;
            self.allow_aac = true;
        }
        if !COOKIE_SOURCES.contains(&self.cookie_source.as_str()) {
            self.cookie_source = "none".to_owned();
        }
        if !matches!(self.yt_dlp_channel.as_str(), "stable" | "nightly") {
            self.yt_dlp_channel = "stable".to_owned();
        }
        if self.theme_id.trim().is_empty() {
            self.theme_id = "system".to_owned();
        }
        self
    }

    /// Why this value cannot be stored, if it cannot. Unlike `clamped`, it
    /// refuses rather than repairs: an edit the user made must not silently
    /// become something else.
    fn validate(&self) -> Result<(), AppError> {
        let ranges: [(&str, u32, u32, u32); 6] = [
            ("maxConcurrent", self.max_concurrent, 1, 20),
            ("segmentsPerItem", self.segments_per_item, 1, 64),
            ("globalMaxConnections", self.global_max_connections, 1, 256),
            (
                "maxConnectionsPerHost",
                self.max_connections_per_host,
                1,
                64,
            ),
            ("minSegmentSizeMb", self.min_segment_size_mb, 1, 100),
            ("maxPlaylistVideos", self.max_playlist_videos, 10, 200),
        ];
        for (name, value, min, max) in ranges {
            if !(min..=max).contains(&value) {
                return Err(AppError::invalid(
                    name,
                    format!("must be between {min} and {max}"),
                ));
            }
        }
        if !RESOLUTION_LADDER.contains(&self.yt_max_height) {
            return Err(AppError::invalid(
                "ytMaxHeight",
                "not a resolution on offer",
            ));
        }
        if !COOKIE_SOURCES.contains(&self.cookie_source.as_str()) {
            return Err(AppError::invalid("cookieSource", "unknown browser"));
        }
        if !matches!(self.yt_dlp_channel.as_str(), "stable" | "nightly") {
            return Err(AppError::invalid("ytDlpChannel", "unknown channel"));
        }
        if !(self.allow_av1 || self.allow_vp9 || self.allow_h264) {
            return Err(AppError::invalid(
                "allowAv1",
                "keep at least one video codec",
            ));
        }
        if !(self.allow_mp4 || self.allow_webm) {
            return Err(AppError::invalid("allowMp4", "keep at least one container"));
        }
        if !(self.allow_opus || self.allow_aac) {
            return Err(AppError::invalid(
                "allowOpus",
                "keep at least one audio codec",
            ));
        }
        if self.theme_id.trim().is_empty() {
            return Err(AppError::invalid("themeId", "the theme is empty"));
        }
        Ok(())
    }

    pub fn cookie_source(&self) -> CookieSource {
        serde_json::from_value(serde_json::Value::String(self.cookie_source.clone()))
            .unwrap_or(CookieSource::None)
    }

    pub fn channel(&self) -> YtDlpChannel {
        match self.yt_dlp_channel.as_str() {
            "nightly" => YtDlpChannel::Nightly,
            _ => YtDlpChannel::Stable,
        }
    }

    pub fn quality_preferences(&self) -> QualityPreferences {
        QualityPreferences {
            max_height: i64::from(self.yt_max_height),
            video_codecs: [
                (self.allow_av1, VideoCodec::Av1),
                (self.allow_vp9, VideoCodec::Vp9),
                (self.allow_h264, VideoCodec::H264),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, codec)| codec)
            .collect(),
            containers: [
                (self.allow_mp4, MediaContainer::Mp4),
                (self.allow_webm, MediaContainer::Webm),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, container)| container)
            .collect(),
            audio_codecs: [
                (self.allow_opus, AudioCodec::Opus),
                (self.allow_aac, AudioCodec::Aac),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, codec)| codec)
            .collect(),
        }
    }
}

/// The file's settings, tolerating bad fields one at a time: a field that
/// does not parse keeps its default and the rest still load.
fn lenient(data: &[u8]) -> SettingsDto {
    let Ok(serde_json::Value::Object(file)) = serde_json::from_slice::<serde_json::Value>(data)
    else {
        return SettingsDto::default();
    };
    let mut merged = match serde_json::to_value(SettingsDto::default()) {
        Ok(serde_json::Value::Object(defaults)) => defaults,
        _ => return SettingsDto::default(),
    };
    for (key, value) in file {
        let previous = merged.insert(key.clone(), value);
        let trial = serde_json::Value::Object(merged.clone());
        if serde_json::from_value::<SettingsDto>(trial).is_err() {
            match previous {
                Some(old) => merged.insert(key, old),
                None => merged.remove(&key),
            };
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default()
}

pub struct SettingsStore {
    path: PathBuf,
    /// `download_folder` is always `Some` and validated.
    current: SettingsDto,
    shared: SharedSettings,
}

impl SettingsStore {
    /// Reads `path`; a missing or unreadable file gives the defaults.
    pub fn load(path: PathBuf, default_folder: PathBuf) -> Self {
        let mut current = std::fs::read(&path)
            .ok()
            .map(|data| lenient(&data))
            .unwrap_or_default()
            .clamped();
        let folder = current
            .download_folder
            .as_deref()
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .unwrap_or(default_folder);
        current.download_folder = Some(folder.to_string_lossy().into_owned());
        let shared = Arc::new(RwLock::new(current.clone()));
        Self {
            path,
            current,
            shared,
        }
    }

    /// The live value other components read.
    pub fn shared(&self) -> SharedSettings {
        Arc::clone(&self.shared)
    }

    pub fn download_folder(&self) -> PathBuf {
        PathBuf::from(self.current.download_folder.clone().unwrap_or_default())
    }

    pub fn get(&self) -> SettingsDto {
        self.current.clone()
    }

    /// Validates, persists and applies. On any error nothing changes.
    pub fn update(&mut self, dto: SettingsDto) -> Result<(), AppError> {
        dto.validate()?;
        let folder = match dto.download_folder.as_deref() {
            Some(text) => validate_folder(text)?.to_string_lossy().into_owned(),
            None => self.current.download_folder.clone().unwrap_or_default(),
        };
        let next = SettingsDto {
            download_folder: Some(folder),
            ..dto
        };
        self.write(&next)?;
        self.current = next.clone();
        *self
            .shared
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
        Ok(())
    }

    fn write(&self, stored: &SettingsDto) -> Result<(), AppError> {
        let data = serde_json::to_vec_pretty(stored)
            .map_err(|e| AppError::failed(format!("could not encode the settings: {e}")))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::failed(format!("could not create {}: {e}", parent.display()))
            })?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, data)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .map_err(|e| AppError::failed(format!("could not save the settings: {e}")))
    }
}

/// The folder must be a non-empty absolute path that exists (or can be
/// created) and accepts a new file.
fn validate_folder(text: &str) -> Result<PathBuf, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("downloadFolder", "the folder is empty"));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::invalid(
            "downloadFolder",
            "the folder must be an absolute path",
        ));
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| AppError::invalid("downloadFolder", format!("cannot use that folder: {e}")))?;
    if !path.is_dir() {
        return Err(AppError::invalid(
            "downloadFolder",
            "that path is not a folder",
        ));
    }
    probe_writable(&path)?;
    Ok(path)
}

fn probe_writable(folder: &Path) -> Result<(), AppError> {
    let probe = folder.join(format!(".sdm-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| {
        AppError::invalid(
            "downloadFolder",
            format!("that folder is not writable: {e}"),
        )
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> SettingsStore {
        SettingsStore::load(
            dir.path().join("settings.json"),
            dir.path().join("Downloads"),
        )
    }

    fn with_folder(folder: impl Into<String>) -> SettingsDto {
        SettingsDto {
            download_folder: Some(folder.into()),
            ..SettingsDto::default()
        }
    }

    #[test]
    fn a_missing_file_gives_the_default_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(store.download_folder(), dir.path().join("Downloads"));
    }

    #[test]
    fn a_valid_update_is_persisted_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        let mut s = store(&dir);
        s.update(with_folder(target.to_string_lossy())).unwrap();
        assert!(target.is_dir(), "the folder is created");
        let reloaded = store(&dir);
        assert_eq!(reloaded.download_folder(), target);
    }

    #[test]
    fn invalid_folders_are_rejected_and_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        for bad in ["", "   ", "relative/path", file.to_str().unwrap()] {
            let error = s.update(with_folder(bad)).expect_err(bad);
            assert!(
                matches!(error, AppError::InvalidArgument { .. }),
                "{bad}: {error:?}"
            );
        }
        assert_eq!(
            s.download_folder(),
            dir.path().join("Downloads"),
            "old setting still in force"
        );
    }

    #[test]
    fn a_corrupt_file_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        assert_eq!(store(&dir).download_folder(), dir.path().join("Downloads"));
    }

    #[test]
    fn an_old_file_with_only_the_folder_loads_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("kept");
        std::fs::write(
            dir.path().join("settings.json"),
            format!(r#"{{"downloadFolder": {:?}}}"#, folder.to_string_lossy()),
        )
        .unwrap();
        let s = store(&dir);
        assert_eq!(s.download_folder(), folder);
        let got = s.get();
        assert_eq!(
            SettingsDto {
                download_folder: got.download_folder.clone(),
                ..SettingsDto::default()
            },
            got
        );
        assert_eq!(got.max_concurrent, 3);
        assert_eq!(got.theme_id, "system");
    }

    #[test]
    fn out_of_range_numbers_in_the_file_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"downloadFolder": "/x", "maxConcurrent": 0, "segmentsPerItem": 999,
                 "globalMaxConnections": 100000, "maxConnectionsPerHost": 0,
                 "minSegmentSizeMb": 5000, "maxPlaylistVideos": 3}"#,
        )
        .unwrap();
        let got = store(&dir).get();
        assert_eq!(got.max_concurrent, 1);
        assert_eq!(got.segments_per_item, 64);
        assert_eq!(got.global_max_connections, 256);
        assert_eq!(got.max_connections_per_host, 1);
        assert_eq!(got.min_segment_size_mb, 100);
        assert_eq!(got.max_playlist_videos, 10);
    }

    #[test]
    fn an_unknown_cookie_source_or_channel_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"downloadFolder": "/x", "cookieSource": "netscape", "ytDlpChannel": "beta",
                 "autoClearInterval": "fortnight", "activationMode": "sideways"}"#,
        )
        .unwrap();
        let got = store(&dir).get();
        assert_eq!(got.cookie_source, "none");
        assert_eq!(got.yt_dlp_channel, "stable");
        assert_eq!(got.auto_clear_interval, AutoClearInterval::ThirtyMinutes);
        assert_eq!(got.activation_mode, ActivationMode::Both);
    }

    #[test]
    fn an_emptied_codec_set_is_restored_to_all() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"downloadFolder": "/x", "allowAv1": false, "allowVp9": false, "allowH264": false,
                 "allowMp4": false, "allowWebm": false, "allowOpus": false, "allowAac": false}"#,
        )
        .unwrap();
        let got = store(&dir).get();
        assert!(got.allow_av1 && got.allow_vp9 && got.allow_h264);
        assert!(got.allow_mp4 && got.allow_webm);
        assert!(got.allow_opus && got.allow_aac);
    }

    #[test]
    fn the_resolution_snaps_onto_the_ladder() {
        for (stored, expected) in [(1000, 720), (5000, 1920), (300, 480), (1080, 1080)] {
            let settings = SettingsDto {
                yt_max_height: stored,
                ..SettingsDto::default()
            };
            assert_eq!(settings.clamped().yt_max_height, expected, "{stored}");
        }
    }

    #[test]
    fn an_empty_theme_id_becomes_system() {
        let settings = SettingsDto {
            theme_id: "  ".into(),
            ..SettingsDto::default()
        };
        assert_eq!(settings.clamped().theme_id, "system");
    }

    #[test]
    fn update_rejects_out_of_range_values_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let before = s.get();
        let bad: Vec<(&str, SettingsDto)> = vec![
            (
                "maxConcurrent",
                SettingsDto {
                    max_concurrent: 21,
                    ..Default::default()
                },
            ),
            (
                "maxConcurrent",
                SettingsDto {
                    max_concurrent: 0,
                    ..Default::default()
                },
            ),
            (
                "segmentsPerItem",
                SettingsDto {
                    segments_per_item: 65,
                    ..Default::default()
                },
            ),
            (
                "globalMaxConnections",
                SettingsDto {
                    global_max_connections: 257,
                    ..Default::default()
                },
            ),
            (
                "maxConnectionsPerHost",
                SettingsDto {
                    max_connections_per_host: 65,
                    ..Default::default()
                },
            ),
            (
                "minSegmentSizeMb",
                SettingsDto {
                    min_segment_size_mb: 101,
                    ..Default::default()
                },
            ),
            (
                "maxPlaylistVideos",
                SettingsDto {
                    max_playlist_videos: 9,
                    ..Default::default()
                },
            ),
            (
                "ytMaxHeight",
                SettingsDto {
                    yt_max_height: 1000,
                    ..Default::default()
                },
            ),
            (
                "cookieSource",
                SettingsDto {
                    cookie_source: "netscape".into(),
                    ..Default::default()
                },
            ),
            (
                "ytDlpChannel",
                SettingsDto {
                    yt_dlp_channel: "beta".into(),
                    ..Default::default()
                },
            ),
            (
                "allowAv1",
                SettingsDto {
                    allow_av1: false,
                    allow_vp9: false,
                    allow_h264: false,
                    ..Default::default()
                },
            ),
            (
                "allowMp4",
                SettingsDto {
                    allow_mp4: false,
                    allow_webm: false,
                    ..Default::default()
                },
            ),
            (
                "allowOpus",
                SettingsDto {
                    allow_opus: false,
                    allow_aac: false,
                    ..Default::default()
                },
            ),
        ];
        for (field, dto) in bad {
            match s.update(dto).expect_err(field) {
                AppError::InvalidArgument { argument, .. } => assert_eq!(argument, field),
                other => panic!("{field}: {other:?}"),
            }
        }
        assert_eq!(s.get(), before);
        assert_eq!(store(&dir).get(), before, "nothing was written");
    }

    #[test]
    fn update_without_a_folder_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.update(SettingsDto {
            max_concurrent: 7,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s.download_folder(), dir.path().join("Downloads"));
        assert_eq!(store(&dir).get().max_concurrent, 7);
    }

    #[test]
    fn update_persists_and_reloads_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let changed = SettingsDto {
            download_folder: None,
            max_concurrent: 5,
            segments_per_item: 16,
            global_max_connections: 64,
            max_connections_per_host: 4,
            min_segment_size_mb: 20,
            auto_start_downloads_on_launch: true,
            clipboard_watching_enabled: false,
            auto_add_and_start_on_grab: true,
            deep_sniff_enabled: false,
            auto_clear_interval: AutoClearInterval::Never,
            yt_max_height: 720,
            allow_av1: false,
            allow_vp9: true,
            allow_h264: true,
            allow_mp4: true,
            allow_webm: false,
            allow_opus: false,
            allow_aac: true,
            max_playlist_videos: 120,
            cookie_source: "firefox".into(),
            yt_dlp_channel: "nightly".into(),
            notify_download_finished: false,
            notify_package_finished: false,
            notify_download_failed: false,
            notify_links_grabbed: false,
            launch_at_login: true,
            activation_mode: ActivationMode::DockOnly,
            theme_id: "deep-purple".into(),
        };
        s.update(changed.clone()).unwrap();
        let expected = SettingsDto {
            download_folder: s.get().download_folder,
            ..changed
        };
        assert_eq!(s.get(), expected);
        assert_eq!(store(&dir).get(), expected);
    }

    #[test]
    fn the_shared_value_follows_an_update() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let shared = s.shared();
        s.update(SettingsDto {
            max_concurrent: 9,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(shared.read().unwrap().max_concurrent, 9);
    }

    #[test]
    fn auto_clear_durations_match_the_swiftui_intervals() {
        assert_eq!(
            AutoClearInterval::FifteenMinutes.duration(),
            Some(std::time::Duration::from_secs(900))
        );
        assert_eq!(
            AutoClearInterval::OneDay.duration(),
            Some(std::time::Duration::from_secs(86_400))
        );
        assert_eq!(AutoClearInterval::Never.duration(), None);
    }
}
