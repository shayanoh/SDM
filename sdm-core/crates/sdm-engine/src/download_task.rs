use thiserror::Error;

/// Task 14 adds the rest of `DownloadTask.swift` to this module; the error
/// enum lives here first because `RetryPolicy` classifies it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error("the server did not report a size")]
    UnknownSize,
    #[error("server error: HTTP {status}")]
    ServerError { status: u16 },
    /// A `403`/`410` on a component whose URL can be refreshed by a
    /// `LinkResolver` (a signed `googlevideo` URL that has expired). Carries
    /// the yt-dlp `format_id` the engine needs to request a fresh URL.
    #[error("the URL for format {format_id} expired")]
    UrlExpired { format_id: String },
    /// The resolver could not hand back a usable fresh URL for an expired
    /// component — the format is gone, its size changed, or the resolve
    /// itself failed. Terminal.
    #[error("refreshing the URL failed: {reason}")]
    RefreshFailed { reason: String },
    #[error("workers finished but the download is incomplete")]
    IncompleteAfterWorkersFinished,
    /// The origin's response body ended before the claimed range was fully
    /// delivered, without the transport throwing — a clean short read. Bytes
    /// already written for the claim remain on disk and recorded in
    /// `completed_ranges`; only the unwritten remainder is lost.
    #[error("truncated response: expected {expected} bytes, received {received}")]
    TruncatedResponse { expected: i64, received: i64 },
}
