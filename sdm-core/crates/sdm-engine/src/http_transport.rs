use thiserror::Error;

/// How a fetch failed. Task 13 adds the rest of `HTTPTransport.swift`
/// (`RangeRequest`, `RangeResponse`, the `HttpTransport` trait) to this
/// module; the error enum lives here first because `RetryPolicy` classifies it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransportError {
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("HTTP {status}")]
    Http { status: u16 },
    #[error("malformed response")]
    MalformedResponse,
}
