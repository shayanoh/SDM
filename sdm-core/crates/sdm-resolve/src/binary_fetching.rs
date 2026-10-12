use std::time::Duration;

use async_trait::async_trait;
use url::Url;

/// Twin of Swift `BinaryFetchError`. `Display` matches Swift's string
/// interpolation of the enum (`http(404)`, `transport("offline")`) because
/// `ManagedBinaries` persists that text in the manifest's `lastError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BinaryFetchError {
    #[error("http({0})")]
    Http(i64),
    #[error("transport({0:?})")]
    Transport(String),
}

/// The network seam for `ManagedBinaries`. Tests inject a fake so the update
/// path never touches the network.
#[async_trait]
pub trait BinaryFetching: Send + Sync {
    /// GETs `url`, following redirects, and returns the body. Returns
    /// `BinaryFetchError::Http` on a non-2xx status and
    /// `BinaryFetchError::Transport` on a connection failure.
    async fn data(&self, url: &Url) -> Result<Vec<u8>, BinaryFetchError>;
}

/// `URLRequest.timeoutInterval` in Swift: 60 s.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Twin of Swift `URLSessionBinaryFetcher`, on `reqwest` with rustls and the
/// operating system's trust store.
///
/// `URLRequest.timeoutInterval = 60` is an idle timeout (time without
/// data) and also bounds connecting, so this uses reqwest's `connect_timeout`
/// and `read_timeout` rather than `timeout`; the 35 MB `yt-dlp` download must
/// not be cut off at 60 s.
pub struct ReqwestBinaryFetcher {
    /// A failed client build (TLS initialisation) is reported by every
    /// `data` call as a transport error, as `URLSession` would fail the
    /// request, instead of panicking at construction.
    client: Result<reqwest::Client, String>,
}

impl ReqwestBinaryFetcher {
    pub fn new() -> Self {
        Self::with_builder(reqwest::Client::builder())
    }

    /// Applies the `SDM` user agent and the 60 s connect and idle timeouts to `builder`.
    /// Lets tests supply, for example, a builder with proxies disabled.
    pub fn with_builder(builder: reqwest::ClientBuilder) -> Self {
        let client = builder
            .user_agent("SDM")
            .connect_timeout(IDLE_TIMEOUT)
            .read_timeout(IDLE_TIMEOUT)
            .build()
            .map_err(|e| e.to_string());
        Self { client }
    }
}

impl Default for ReqwestBinaryFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BinaryFetching for ReqwestBinaryFetcher {
    async fn data(&self, url: &Url) -> Result<Vec<u8>, BinaryFetchError> {
        let client = self
            .client
            .as_ref()
            .map_err(|e| BinaryFetchError::Transport(e.clone()))?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| BinaryFetchError::Transport(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(BinaryFetchError::Http(i64::from(status.as_u16())));
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| BinaryFetchError::Transport(e.to_string()))?;
        Ok(body.to_vec())
    }
}
