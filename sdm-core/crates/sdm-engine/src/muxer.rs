//! Joins a video-only and an audio-only part with `ffmpeg`. Port of
//! `Muxer.swift`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sdm_core::MediaContainer;
use sdm_resolve::{BinaryLocator, ProcessRunError, ProcessRunner};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// How many trailing characters of `ffmpeg`'s stderr a failure carries.
const STDERR_TAIL_CHARACTERS: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MuxError {
    #[error("ffmpeg is missing")]
    FfmpegMissing,
    #[error("ffmpeg failed: {stderr_tail}")]
    FfmpegFailed { stderr_tail: String },
    #[error("ffmpeg timed out")]
    TimedOut,
    /// Any other way the process runner failed (the launch, or the call being
    /// cancelled). Swift lets those errors propagate as is; this carries them
    /// so the enum stays `Clone`/`Eq`.
    #[error(transparent)]
    Process(#[from] ProcessRunError),
}

/// Combines a video-only and audio-only part into one container with
/// `ffmpeg -c copy` (no re-encode). Parent spec §7.2.
///
/// `cancel` stands in for Swift's implicit task cancellation: cancelling it
/// terminates `ffmpeg` and the call returns `MuxError::Process(Cancelled)`.
#[async_trait]
pub trait Muxer: Send + Sync {
    async fn mux(
        &self,
        video_part: &Path,
        audio_part: &Path,
        output: &Path,
        container: &MediaContainer,
        cancel: &CancellationToken,
    ) -> Result<(), MuxError>;
}

pub struct FfmpegMuxer {
    runner: Arc<dyn ProcessRunner>,
    locator: Arc<BinaryLocator>,
    timeout: Duration,
}

impl FfmpegMuxer {
    /// A muxer with Swift's default timeout of 120 seconds.
    pub fn new(runner: Arc<dyn ProcessRunner>, locator: Arc<BinaryLocator>) -> Self {
        Self {
            runner,
            locator,
            timeout: Duration::from_secs(120),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn arguments(
        video: &Path,
        audio: &Path,
        output: &Path,
        container: &MediaContainer,
    ) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            video.to_string_lossy().into_owned(),
            "-i".to_string(),
            audio.to_string_lossy().into_owned(),
            "-c".to_string(),
            "copy".to_string(),
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "1:a:0".to_string(),
        ];
        if *container == MediaContainer::Mp4 {
            args.push("-movflags".to_string());
            args.push("+faststart".to_string());
        }
        args.push(output.to_string_lossy().into_owned());
        args
    }
}

#[async_trait]
impl Muxer for FfmpegMuxer {
    async fn mux(
        &self,
        video_part: &Path,
        audio_part: &Path,
        output: &Path,
        container: &MediaContainer,
        cancel: &CancellationToken,
    ) -> Result<(), MuxError> {
        let ffmpeg = self
            .locator
            .locate("ffmpeg")
            .ok_or(MuxError::FfmpegMissing)?;
        let out = self
            .runner
            .run(
                &ffmpeg,
                &Self::arguments(video_part, audio_part, output, container),
                self.timeout,
                cancel,
            )
            .await
            .map_err(|error| match error {
                ProcessRunError::TimedOut => MuxError::TimedOut,
                other => MuxError::Process(other),
            })?;
        if out.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(MuxError::FfmpegFailed {
                stderr_tail: tail(&stderr, STDERR_TAIL_CHARACTERS),
            });
        }
        Ok(())
    }
}

/// Swift's `String.suffix(count)`: the last `count` characters.
fn tail(text: &str, count: usize) -> String {
    let skip = text.chars().count().saturating_sub(count);
    text.chars().skip(skip).collect()
}
