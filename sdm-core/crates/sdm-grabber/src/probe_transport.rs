use std::collections::HashMap;

use async_trait::async_trait;
use sdm_core::{ByteRange, SourceUrl};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeMethod {
    Head,
    Get,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeRequest {
    pub url: SourceUrl,
    pub method: ProbeMethod,
    /// Only meaningful for `Get`: requests a slice via `Range`, or the
    /// whole (bounded) body when `None`.
    pub range: Option<ByteRange>,
}

impl ProbeRequest {
    pub fn new(url: SourceUrl, method: ProbeMethod) -> Self {
        Self {
            url,
            method,
            range: None,
        }
    }

    pub fn with_range(mut self, range: ByteRange) -> Self {
        self.range = Some(range);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResponse {
    pub status_code: i64,
    /// URL after following redirects.
    pub final_url: SourceUrl,
    /// Header names lowercased for case-insensitive lookup.
    pub headers: HashMap<String, String>,
    /// Empty for `Head`; the requested slice (or whole body) for `Get`.
    pub body: Vec<u8>,
}

/// What a probe can fail with. The Swift enum has three cases; any other
/// `URLError` escaped `send` as itself, which here is [`ProbeError::Network`]
/// carrying the failure's whole cause chain.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("the request timed out")]
    TimedOut,
    #[error("the host could not be found")]
    DnsFailure,
    #[error("the response was malformed")]
    MalformedResponse,
    /// Any other transport failure (refused, reset, too many redirects, an
    /// unusable URL, ...).
    #[error("{description}")]
    Network { description: String },
}

/// The grabber's only route to the network. Injected so tests never touch
/// it. Deliberately separate from `sdm_engine::HttpTransport` — `sdm-grabber`
/// depends only on `sdm-core` and `sdm-resolve`.
///
/// There is no cancellation token: the Swift prober is never cancelled.
/// `GrabberSession.cancelChecks()` only stops the session launching further
/// probes, and probes already in flight finish and are recorded, so
/// `send` and `LinkProber::probe` simply run to completion.
#[async_trait]
pub trait ProbeTransport: Send + Sync {
    async fn send(&self, request: ProbeRequest) -> Result<ProbeResponse, ProbeError>;
}
