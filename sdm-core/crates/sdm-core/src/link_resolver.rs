use crate::resolved_media::ResolvedTarget;
use crate::source_url::SourceUrl;
use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshedFormat {
    /// The fresh direct URL, exactly as yt-dlp reported it. It is written
    /// into the item's component and persisted, so it keeps its original
    /// string (a `url::Url` would lowercase the host and normalize it).
    pub url: SourceUrl,
    pub filesize: Option<i64>,
    pub format_id: String,
}

impl RefreshedFormat {
    pub fn new(url: SourceUrl, filesize: Option<i64>, format_id: impl Into<String>) -> Self {
        Self {
            url,
            filesize,
            format_id: format_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveError {
    #[error("yt-dlp binary is missing")]
    BinaryMissing,
    #[error("unsupported link")]
    Unsupported,
    #[error("the requested format is gone")]
    FormatGone,
    #[error("authentication required")]
    AuthRequired,
    #[error("private video")]
    PrivateVideo,
    #[error("unavailable")]
    Unavailable,
    /// The stream is DRM-protected — yt-dlp can see it but only yields
    /// encrypted output. Terminal; the grabber shows it as unsupported.
    #[error("DRM-protected stream")]
    DrmProtected,
    #[error("timed out")]
    Timeout,
    #[error("yt-dlp failed: {stderr_tail}")]
    YtDlpFailed { stderr_tail: String },
    /// Twin of Swift's `CancellationError` escaping `refresh` (the resolver
    /// does not catch it): the `cancel` token passed to `refresh` fired.
    #[error("cancelled")]
    Cancelled,
}

/// The extension seam for extractor-backed sites. Generic HTTP has no
/// resolver — a `None` resolver, or `can_handle == false`, means "use the
/// existing probe/download path." Parent spec §4.1.
#[async_trait]
pub trait LinkResolver: Send + Sync {
    /// Cheap, synchronous, no I/O — a host/path check only. A URL string the
    /// `url` crate cannot parse (`SourceUrl::as_url` is `None`) has no host
    /// to match and is not handled.
    fn can_handle(&self, url: &SourceUrl) -> bool;

    /// Grab-time resolution. May shell out; may take seconds. A URL with no
    /// parsed form is `ResolveError::Unsupported`. The URL's original string
    /// is what reaches yt-dlp.
    async fn resolve(&self, url: &SourceUrl) -> Result<ResolvedTarget, ResolveError>;

    /// Mid-download URL refresh for one expired component: re-resolve
    /// `source_url` (the item's grabbed URL) and hand back a fresh direct URL
    /// for `format_id`.
    ///
    /// `cancel` stands in for Swift's implicit task cancellation: the engine
    /// runs this inside the item's job, so pausing or stopping the item
    /// cancels it and kills the in-flight yt-dlp. A cancelled refresh returns
    /// `ResolveError::Cancelled`. (`resolve` takes no token: nothing in the
    /// Swift app ever cancels one.)
    async fn refresh(
        &self,
        source_url: &SourceUrl,
        format_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RefreshedFormat, ResolveError>;
}
