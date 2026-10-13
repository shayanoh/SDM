//! The production transport, on `reqwest` (rustls, operating-system trust
//! store, no OpenSSL).
//!
//! # What `URLSessionTransport` configures or relies on, and the twin here
//!
//! * **Session kind.** `.ephemeral`: no disk cache, no persisted cookies, but
//!   an in-memory cookie jar for the session's life, so an origin that sets a
//!   cookie on the probe (or on a redirect hop) and expects it back works.
//!   Twin: reqwest has no cache; `cookie_store(true)` gives the same
//!   in-memory jar, shared by every request of this transport and replayed
//!   across redirect hops (pinned by a test). Nothing is persisted.
//! * **Cache policy.** `reloadIgnoringLocalCacheData`, `urlCache = nil`, so a
//!   multi-gigabyte body is never considered for caching. Twin: nothing is
//!   cached and no conditional headers are sent; pinned by tests that fetch
//!   a `max-age` response twice.
//! * **Pipelining.** `httpShouldUsePipelining = false`. Twin: hyper does not
//!   pipeline HTTP/1.1.
//! * **Per-host connection limit.** `httpMaximumConnectionsPerHost = 256`
//!   (the default of 6 throttles segmented downloads; the engine budgets
//!   connections itself). Twin: hyper-util's pool opens a connection per
//!   in-flight request with no per-host cap, and keeps idle ones without a
//!   limit. Over HTTP/2 both clients multiplex on one connection; reqwest's
//!   default 64 KiB stream window would cap per-stream throughput at
//!   window/RTT, so adaptive window sizing is switched on. DIFFERENCE
//!   (deliberate): URLSession negotiates its own window sizes.
//! * **HTTP version.** URLSession negotiates h2 or HTTP/1.1 by ALPN (and may
//!   pick HTTP/3). Twin: h2 or HTTP/1.1 by ALPN (`http2` feature); no
//!   HTTP/3. DIFFERENCE.
//! * **Timeouts.** The ephemeral defaults: `timeoutIntervalForRequest` 60 s
//!   (no data for that long, including while connecting) and
//!   `timeoutIntervalForResource` 7 days. Twin: `connect_timeout` 60 s,
//!   `read_timeout` 60 s (per read, so it is an idle timeout, not a total),
//!   `timeout` 7 days (total).
//! * **Redirects.** URLSession follows redirects itself (its limit is 16,
//!   failing with `tooManyRedirects`), carries the original headers (so
//!   `Range` survives every hop) and sends no `Referer`. The final URL is
//!   `HTTPURLResponse.url`, used as `resolved_url`. Twin:
//!   `Policy::limited(16)`, `referer(false)`, `response.url()` (reqwest keeps
//!   custom headers across hops, dropping only credentials on a cross-origin
//!   hop). Exceeding the limit is a send error, so `ConnectionDropped`.
//! * **Header set.** The request carries only `Range` (when the range is
//!   non-empty) beside URLSession's defaults. Twin: `Range` plus
//!   `User-Agent: SDM` (as `ReqwestBinaryFetcher`). DIFFERENCE: URLSession
//!   sends `Accept-Encoding: gzip, deflate` and decodes transparently;
//!   reqwest (no compression features) sends none, so the origin serves the
//!   identity encoding, which is what byte ranges address anyway.
//! * **System integration.** Proxies, including PAC and per-protocol
//!   settings, come from the system. Twin: the `system-proxy` feature reads
//!   the OS proxy settings; PAC scripts are not evaluated. DIFFERENCE. Trust
//!   comes from the operating-system store via `rustls-platform-verifier`.
//! * **Status codes.** The transport never turns a status into an error: the
//!   response carries `statusCode` and the caller decides. `TransportError::
//!   Http` is therefore never produced here. Twin: same.
//! * **Header interpretation** (static helpers, unit-tested without a
//!   socket): `Range` is inclusive and only sent for a non-empty range; ranges
//!   are accepted on a `206` or `Accept-Ranges: bytes` (case-insensitive);
//!   the validator is `ETag`, else `Last-Modified`; the total size prefers
//!   the `Content-Range` total over `Content-Length`, a `*` total or an
//!   absent length meaning unknown. Twin: the same four functions over a
//!   `HeaderMap`.
//! * **Error mapping.** URLSession throws the raw `URLError` (the retry
//!   policy treats every error it does not recognise as transient), and
//!   `TransportError.malformedResponse` for a non-HTTP response or a
//!   completion without one. Twin: every reqwest failure before or during the
//!   body (refused, reset, timed out, a body shorter than its
//!   `Content-Length`, too many redirects, a parse failure) is
//!   `TransportError::Network { description }`, carrying the whole cause
//!   chain (the `localizedDescription` twin the engine shows after its last
//!   retry); the retry policy treats it as transient, like
//!   `ConnectionDropped` (which remains for a client that failed to build).
//!   `MalformedResponse` is never produced: reqwest only ever yields HTTP
//!   responses, and hyper reports an unparsable one as a send error.
//! * **Cancellation.** The stream's `onTermination` cancels the task, and
//!   cancelling the awaiting task cancels it before headers. Twin: dropping
//!   the body stream drops the hyper response body, which closes the
//!   connection (HTTP/1.1) or resets the stream (h2); dropping the `fetch`
//!   future before headers cancels the request. Both are pinned by tests
//!   against a loopback server that observes the disconnect.
//! * **Back-pressure.** The delegate yields into an unbounded
//!   `AsyncThrowingStream`, so a slow consumer makes memory grow without
//!   limit. Twin: the stream is pulled, so a slow consumer stops reading and
//!   TCP / h2 flow control throttles the origin. DIFFERENCE (an improvement;
//!   buffer sizes also differ: URLSession delivers whatever `Data` it has,
//!   hyper delivers its read buffers).

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, LAST_MODIFIED,
};
use reqwest::redirect::Policy;
use sdm_core::ByteRange;

use crate::http_transport::{HttpTransport, RangeRequest, RangeResponse, TransportError};

/// `URLSession`'s redirect limit.
const MAX_REDIRECTS: usize = 16;
/// `timeoutIntervalForRequest` of an ephemeral session.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// `timeoutIntervalForResource` of an ephemeral session.
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct ReqwestTransport {
    /// A failed client build (TLS initialisation) is reported by every
    /// `fetch` as `ConnectionDropped` instead of panicking at construction.
    client: Option<reqwest::Client>,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self::with_builder(reqwest::Client::builder())
    }

    /// Applies this transport's configuration to `builder`. Lets tests supply,
    /// for example, a builder with proxies disabled.
    pub fn with_builder(builder: reqwest::ClientBuilder) -> Self {
        let client = builder
            .user_agent("SDM")
            .cookie_store(true)
            .redirect(Policy::limited(MAX_REDIRECTS))
            .referer(false)
            .connect_timeout(IDLE_TIMEOUT)
            .read_timeout(IDLE_TIMEOUT)
            .timeout(RESOURCE_TIMEOUT)
            .http2_adaptive_window(true)
            .build();
        match client {
            Ok(client) => Self {
                client: Some(client),
            },
            Err(error) => {
                tracing::error!("could not build the HTTP client: {error}");
                Self { client: None }
            }
        }
    }

    // MARK: - Pure boundary logic (unit-tested without a network)

    /// HTTP `Range` is inclusive on both ends; `ByteRange` is half-open. This
    /// is the only place in the codebase that conversion happens.
    pub fn range_header_value(range: Option<ByteRange>) -> Option<String> {
        let range = range?;
        if range.length() <= 0 {
            return None;
        }
        Some(format!("bytes={}-{}", range.start, range.end - 1))
    }

    pub fn accepts_ranges(status: u16, headers: &HeaderMap) -> bool {
        if status == 206 {
            return true;
        }
        header_text(headers, &ACCEPT_RANGES).is_some_and(|v| v.to_lowercase() == "bytes")
    }

    pub fn validator(headers: &HeaderMap) -> Option<String> {
        header_text(headers, &ETAG).or_else(|| header_text(headers, &LAST_MODIFIED))
    }

    /// Prefers the total from `Content-Range` (correct for partial responses)
    /// over `Content-Length` (which reports only the slice).
    pub fn total_size(headers: &HeaderMap) -> Option<i64> {
        if let Some(content_range) = header_text(headers, &CONTENT_RANGE)
            && let Some(slash) = content_range.rfind('/')
        {
            let total = &content_range[slash + 1..];
            if total != "*"
                && let Ok(value) = total.parse::<i64>()
            {
                return Some(value);
            }
        }
        header_text(headers, &CONTENT_LENGTH)
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|length| *length >= 0)
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// The header's value as text. A value that is not visible ASCII (a server
/// sending raw UTF-8 or Latin-1) is never dropped: it decodes as UTF-8 when
/// it is valid UTF-8, otherwise as Latin-1 (each byte is the code point of
/// the same number).
fn header_text(headers: &HeaderMap, name: &reqwest::header::HeaderName) -> Option<String> {
    let bytes = headers.get(name)?.as_bytes();
    Some(match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
    })
}

/// Wraps a reqwest failure with its whole cause chain ("error sending request
/// for url (..): client error (Connect): tcp connect error: Connection
/// refused"), since reqwest's own `Display` stops at the outermost layer.
fn network_error(error: reqwest::Error) -> TransportError {
    use std::error::Error;
    let mut description = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        description.push_str(": ");
        description.push_str(&cause.to_string());
        source = cause.source();
    }
    TransportError::Network { description }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let client = self
            .client
            .as_ref()
            .ok_or(TransportError::ConnectionDropped)?;
        let mut builder = client.get(request.url.clone());
        if let Some(value) = Self::range_header_value(request.range) {
            builder = builder.header(reqwest::header::RANGE, value);
        }
        let response = builder.send().await.map_err(network_error)?;

        let status_code = response.status().as_u16();
        let headers = response.headers().clone();
        let resolved_url = response.url().clone();
        let body = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(network_error));

        Ok(RangeResponse {
            status_code,
            total_size: Self::total_size(&headers),
            accepts_ranges: Self::accepts_ranges(status_code, &headers),
            validator: Self::validator(&headers),
            resolved_url,
            body: Box::pin(body),
        })
    }
}
