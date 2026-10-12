use std::fmt;
use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use sdm_core::ByteRange;
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone)]
pub struct RangeRequest {
    pub url: Url,
    /// Half-open range to request, or `None` for the whole resource.
    pub range: Option<ByteRange>,
}

impl RangeRequest {
    pub fn new(url: Url, range: Option<ByteRange>) -> Self {
        Self { url, range }
    }
}

/// The response body: whole buffers, ending cleanly or with the error that
/// cut it short. Dropping the stream cancels the underlying request.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send>>;

pub struct RangeResponse {
    pub status_code: u16,
    /// Total size of the whole resource, when the origin discloses it.
    pub total_size: Option<i64>,
    pub accepts_ranges: bool,
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    pub validator: Option<String>,
    /// The URL the response actually came from, after redirects. A
    /// redirecting origin (e.g. a mirror-network frontend) can send
    /// different requests to different backends, so this is what a caller
    /// should reuse for subsequent requests against the *same* resource
    /// instead of re-resolving the original URL each time.
    pub resolved_url: Url,
    pub body: BodyStream,
}

impl fmt::Debug for RangeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RangeResponse")
            .field("status_code", &self.status_code)
            .field("total_size", &self.total_size)
            .field("accepts_ranges", &self.accepts_ranges)
            .field("validator", &self.validator)
            .field("resolved_url", &self.resolved_url)
            .finish_non_exhaustive()
    }
}

/// How a fetch failed. `RetryPolicy` classifies it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransportError {
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("HTTP {status}")]
    Http { status: u16 },
    #[error("malformed response")]
    MalformedResponse,
}

/// The engine's only route to the network. Injected so tests never touch it.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError>;
}
