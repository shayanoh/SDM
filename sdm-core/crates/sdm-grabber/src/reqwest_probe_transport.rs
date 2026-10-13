//! The production probe transport, on `reqwest` (rustls, operating-system
//! trust store, no OpenSSL). The twin of `sdm-engine`'s `ReqwestTransport`,
//! sharing none of its code (`sdm-grabber` does not depend on `sdm-engine`).
//!
//! # What `URLSessionProbeTransport` configures or relies on, and the twin here
//!
//! * **Session kind.** `URLSession.shared` by default (the initializer takes
//!   any session): default configuration, so a shared disk-backed URL cache
//!   and the process-wide persistent `HTTPCookieStorage`. Twin: reqwest has no
//!   cache, and `cookie_store(true)` is an in-memory jar owned by the
//!   transport and replayed across redirect hops. DIFFERENCE (deliberate):
//!   the jar is not shared with other code and is not persisted.
//! * **Cache policy.** `reloadIgnoringLocalCacheData` on every request.
//!   Twin: nothing is cached and no conditional headers are sent.
//! * **Timeouts.** The default configuration's `timeoutIntervalForRequest`
//!   60 s (no data for that long) and `timeoutIntervalForResource` 7 days.
//!   Twin: `connect_timeout` 60 s, `read_timeout` 60 s, `timeout` 7 days (the
//!   same values as the engine transport).
//! * **Redirects.** URLSession follows up to 16 redirects, carrying the
//!   original headers (so `Range` survives every hop), and reports the last
//!   URL as `HTTPURLResponse.url` (falling back to the request URL).
//!   Twin: `Policy::limited(16)`, `referer(false)`, `response.url()`.
//!   Exceeding the limit is a send error, so a [`ProbeError::Network`].
//! * **HTTP version.** URLSession negotiates h2 or HTTP/1.1 (maybe HTTP/3).
//!   Twin: h2 or HTTP/1.1 by ALPN, no HTTP/3. DIFFERENCE.
//! * **Header set.** The request carries only `Range` (when the range is
//!   non-empty) beside URLSession's defaults. Twin: `Range` plus
//!   `User-Agent: SDM`. DIFFERENCE: URLSession also sends
//!   `Accept-Encoding: gzip, deflate` and decodes transparently; reqwest (no
//!   compression features) sends none, so the origin answers with the
//!   identity encoding, which is also what a magic-byte sniff wants.
//! * **System integration.** Proxies come from the system settings. Twin: the
//!   `system-proxy` feature; PAC scripts are not evaluated. DIFFERENCE.
//! * **Method and range.** `HEAD` or `GET`; `Range: bytes=start-(end-1)` is
//!   sent for any request whose range is non-empty (half-open `ByteRange`
//!   to the inclusive HTTP form), and not otherwise. Twin: the same, for both
//!   methods.
//! * **Body cap.** The bytes are read one at a time and the loop stops as
//!   soon as `count >= maxBytes`, where `maxBytes` is the range's length, or
//!   0 without a range. Because the check follows the append, a `GET` reads at
//!   least one byte (a `GET` with no range, or an empty one, returns exactly
//!   one byte) and at most `max(1, length)`. Twin: chunks are appended and
//!   the result is cut to `max(1, length)`; the response is then dropped,
//!   which closes the connection. A `HEAD` has no body (URLSession's byte
//!   stream is empty), so the body is not read at all.
//! * **Status codes.** The transport never turns a status into an error: the
//!   response carries `statusCode` and the prober decides. Twin: same.
//! * **Headers.** `allHeaderFields` keys are lowercased and values that are
//!   not strings are skipped; URLSession joins repeated headers with `, `.
//!   Twin: names are lowercase already in a `HeaderMap`; repeated values are
//!   joined with `, `; a value that is not visible ASCII is skipped.
//! * **Error mapping.** `ProbeError`s pass through, a `URLError` of
//!   `.timedOut` becomes `timedOut`, `.cannotFindHost` / `.dnsLookupFailed`
//!   become `dnsFailure`, and any other `URLError` escapes as itself.
//!   `malformedResponse` is for a non-HTTP response. Twin: a reqwest timeout
//!   (before or during the body) is `TimedOut`; a failure whose cause chain
//!   holds hyper's `dns error` is `DnsFailure`; everything else is
//!   `Network { description }` carrying the whole cause chain. Nothing here
//!   produces `MalformedResponse`: reqwest only yields HTTP responses and an
//!   unparsable one is a send error.
//! * **Cancellation.** The Swift prober is never cancelled (see
//!   [`ProbeTransport`]); dropping the `send` future cancels the request.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Method;
use reqwest::header::{HeaderMap, RANGE};
use reqwest::redirect::Policy;
use sdm_core::{ByteRange, SourceUrl};

use crate::probe_transport::{
    ProbeError, ProbeMethod, ProbeRequest, ProbeResponse, ProbeTransport,
};

/// `URLSession`'s redirect limit.
const MAX_REDIRECTS: usize = 16;
/// `timeoutIntervalForRequest` of the default configuration.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// `timeoutIntervalForResource` of the default configuration.
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct ReqwestProbeTransport {
    /// A failed client build (TLS initialisation) is reported by every
    /// `send` as a [`ProbeError::Network`] instead of panicking at
    /// construction.
    client: Result<reqwest::Client, String>,
}

impl ReqwestProbeTransport {
    pub fn new() -> Self {
        Self::with_builder(reqwest::Client::builder())
    }

    /// Applies this transport's configuration to `builder`. Lets tests
    /// supply, for example, a builder with proxies disabled.
    pub fn with_builder(builder: reqwest::ClientBuilder) -> Self {
        Self::with_builder_and_idle_timeout(builder, IDLE_TIMEOUT)
    }

    /// As [`Self::with_builder`], with the connect and idle (per-read)
    /// timeouts replaced, so a test can provoke a timeout quickly.
    pub fn with_builder_and_idle_timeout(
        builder: reqwest::ClientBuilder,
        idle_timeout: Duration,
    ) -> Self {
        let client = builder
            .user_agent("SDM")
            .cookie_store(true)
            .redirect(Policy::limited(MAX_REDIRECTS))
            .referer(false)
            .connect_timeout(idle_timeout)
            .read_timeout(idle_timeout)
            .timeout(RESOURCE_TIMEOUT)
            .http2_adaptive_window(true)
            .build();
        match client {
            Ok(client) => Self { client: Ok(client) },
            Err(error) => {
                tracing::error!("could not build the HTTP client: {error}");
                Self {
                    client: Err(error_chain(&error)),
                }
            }
        }
    }

    // MARK: - Pure boundary logic

    /// HTTP `Range` is inclusive on both ends; `ByteRange` is half-open.
    /// Only a non-empty range produces a header.
    pub fn range_header_value(range: Option<ByteRange>) -> Option<String> {
        let range = range?;
        if range.end - range.start <= 0 {
            return None;
        }
        Some(format!("bytes={}-{}", range.start, range.end - 1))
    }

    /// Header names are lowercase in a `HeaderMap`; repeated headers are
    /// joined with `, `. A value that is not visible ASCII (a raw UTF-8 or
    /// Latin-1 `Content-Disposition` filename) is never dropped: it decodes
    /// as UTF-8 when valid, otherwise as Latin-1 (each byte is the code
    /// point of the same number).
    pub fn lowercased_headers(headers: &HeaderMap) -> std::collections::HashMap<String, String> {
        let mut result = std::collections::HashMap::new();
        for name in headers.keys() {
            let values: Vec<String> = headers
                .get_all(name)
                .iter()
                .map(|value| {
                    let bytes = value.as_bytes();
                    match std::str::from_utf8(bytes) {
                        Ok(text) => text.to_owned(),
                        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
                    }
                })
                .collect();
            if !values.is_empty() {
                result.insert(name.as_str().to_lowercase(), values.join(", "));
            }
        }
        result
    }

    /// `URLError.Code` → `ProbeError` for the two failures the prober tells
    /// apart; everything else keeps its cause as [`ProbeError::Network`].
    pub fn classify(error: &reqwest::Error) -> ProbeError {
        if error.is_timeout() {
            return ProbeError::TimedOut;
        }
        if has_dns_cause(error) {
            return ProbeError::DnsFailure;
        }
        ProbeError::Network {
            description: error_chain(error),
        }
    }
}

impl Default for ReqwestProbeTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Walks the cause chain for hyper-util's resolver failure ("dns error").
fn has_dns_cause(error: &reqwest::Error) -> bool {
    use std::error::Error;
    let mut source = error.source();
    while let Some(cause) = source {
        if cause.to_string().starts_with("dns error") {
            return true;
        }
        source = cause.source();
    }
    false
}

/// A reqwest failure with its whole cause chain ("error sending request for
/// url (..): client error (Connect): tcp connect error: Connection refused"),
/// since reqwest's own `Display` stops at the outermost layer.
fn error_chain(error: &reqwest::Error) -> String {
    use std::error::Error;
    let mut description = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        description.push_str(": ");
        description.push_str(&cause.to_string());
        source = cause.source();
    }
    description
}

#[async_trait]
impl ProbeTransport for ReqwestProbeTransport {
    async fn send(&self, request: ProbeRequest) -> Result<ProbeResponse, ProbeError> {
        let client = self
            .client
            .as_ref()
            .map_err(|description| ProbeError::Network {
                description: description.clone(),
            })?;
        let url = request
            .url
            .as_url()
            .ok_or_else(|| ProbeError::Network {
                description: format!("unusable URL: {}", request.url),
            })?
            .clone();
        let method = match request.method {
            ProbeMethod::Head => Method::HEAD,
            ProbeMethod::Get => Method::GET,
        };
        let mut builder = client.request(method, url);
        if let Some(value) = Self::range_header_value(request.range) {
            builder = builder.header(RANGE, value);
        }
        let response = builder.send().await.map_err(|e| Self::classify(&e))?;

        let status_code = i64::from(response.status().as_u16());
        let headers = Self::lowercased_headers(response.headers());
        let final_url = SourceUrl::from_url(response.url());

        let mut body = Vec::new();
        if request.method == ProbeMethod::Get {
            // `maxBytes` is 0 without a range; the loop still takes a byte.
            let limit = request
                .range
                .map_or(0, |range| range.end - range.start)
                .max(1) as usize;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Self::classify(&e))?;
                body.extend_from_slice(&chunk);
                if body.len() >= limit {
                    body.truncate(limit);
                    break;
                }
            }
        }

        Ok(ProbeResponse {
            status_code,
            final_url,
            headers,
            body,
        })
    }
}
