use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholesalePhase {
    Downloading,
    PostProcessing,
}

/// One progress report from a wholesale (yt-dlp-as-downloader) job.
///
/// `total_bytes` may be `None` for a while (some extractors report only an
/// estimate, or nothing until the first fragment). `fraction` is yt-dlp's
/// own percentage, used when byte counts are unavailable. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.5.
#[derive(Debug, Clone, PartialEq)]
pub struct WholesaleProgress {
    pub downloaded_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub fraction: Option<f64>,
    pub phase: WholesalePhase,
    /// True when this report came from yt-dlp's native fragment downloader
    /// (`m3u8_native` / `http_dash_segments`) — i.e. it carried a positive
    /// `fragment_count`. That downloader is the one whose `--continue`
    /// resume SDM relies on, so the engine only advertises a wholesale
    /// component as resumable once it has seen a fragmented report. A run
    /// routed through ffmpeg-as-downloader never sets this. See
    /// `2026-09-03-wholesale-resume-design.md`.
    pub is_fragmented: bool,
}

impl Default for WholesaleProgress {
    /// Swift's initializer defaults: nothing known, `Downloading`, not
    /// fragmented.
    fn default() -> Self {
        Self {
            downloaded_bytes: None,
            total_bytes: None,
            fraction: None,
            phase: WholesalePhase::Downloading,
            is_fragmented: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WholesaleError {
    #[error("yt-dlp binary is missing")]
    BinaryMissing,
    #[error("cancelled")]
    Cancelled,
    #[error("authentication required")]
    AuthRequired,
    #[error("unavailable")]
    Unavailable,
    #[error("yt-dlp failed: {stderr_tail}")]
    Failed { stderr_tail: String },
}

/// Runs an external downloader (yt-dlp) for a stream that has no single
/// `Range`-capable URL. Injected into `DownloadEngine` exactly like
/// `LinkResolver`; `sdm-engine` never constructs one. Parent spec §6.5.
#[async_trait]
pub trait WholesaleDownloader: Send + Sync {
    /// Download `page_url` wholesale to `destination` — a final file, already
    /// muxed. Emits progress. Honors cancellation by terminating the child
    /// process; the caller deletes partial output.
    ///
    /// `cancel` stands in for Swift's implicit task cancellation: cancelling
    /// it terminates the child and the call returns
    /// `WholesaleError::Cancelled`.
    async fn download(
        &self,
        page_url: &Url,
        format_selector: &str,
        destination: &Path,
        on_progress: Arc<dyn Fn(WholesaleProgress) + Send + Sync>,
        cancel: &CancellationToken,
    ) -> Result<(), WholesaleError>;
}
