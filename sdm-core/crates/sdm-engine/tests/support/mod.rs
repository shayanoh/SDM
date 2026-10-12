//! Shared helpers for the engine's integration tests. Each test file pulls
//! this in with `mod support;`, so any one file uses only some of it; the
//! blanket `allow`s below keep the rest from warning there.
#![allow(dead_code, unused_imports)]

mod loopback_server;
mod routing_transport;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use sdm_core::ByteRange;
use sdm_engine::{HttpTransport, RangeRequest, RangeResponse, TransportError};
use tokio::sync::Notify;
use url::Url;

pub use loopback_server::{
    LoopbackBody, LoopbackRequest, LoopbackResponse, LoopbackServer, payload_handler,
};
pub use routing_transport::RoutingTransport;

/// How long a loopback test may take before it fails instead of hanging.
pub const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Fails the test, rather than hanging the suite, if `future` takes longer
/// than [`TEST_TIMEOUT`].
pub async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(TEST_TIMEOUT, future)
        .await
        .expect("timed out: the test hung")
}

/// Deterministic pseudo-random payload; index-derived so any byte can be
/// verified independently of how it was fetched.
pub fn test_payload(count: usize) -> Vec<u8> {
    (0..count)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect()
}

/// A fresh scratch directory, removed when the returned guard drops.
pub fn make_scratch_directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("sdm-tests-")
        .tempdir()
        .expect("create a scratch directory")
}

pub fn test_source_url() -> Url {
    Url::parse("https://example.com/file.bin").expect("a valid URL")
}

/// Reads a response body to its end.
pub async fn collect_body(response: RangeResponse) -> Result<Vec<u8>, TransportError> {
    let mut data = Vec::new();
    let mut body = response.body;
    while let Some(chunk) = body.next().await {
        data.extend_from_slice(&chunk?);
    }
    Ok(data)
}

/// Reads a response body until it ends or fails: the bytes that arrived, and
/// the error that cut the stream short, if any.
pub async fn collect_body_until_error(
    response: RangeResponse,
) -> (Vec<u8>, Option<TransportError>) {
    let mut data = Vec::new();
    let mut body = response.body;
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(error) => return (data, Some(error)),
        }
    }
    (data, None)
}

/// Freezes every non-probe body fetch until `open()` is called, so a test can
/// inspect `DownloadTask::active_worker_count` while every claimed worker is
/// still in flight. `FakeOrigin` has no artificial delay, so an ungated
/// transfer can complete before an assertion runs — this is the fix.
#[derive(Clone, Default)]
pub struct WorkerGate {
    inner: Arc<WorkerGateInner>,
}

#[derive(Default)]
struct WorkerGateInner {
    is_open: AtomicBool,
    notify: Notify,
}

impl WorkerGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn wait_until_open(&self) {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            // Registered before the flag is read, so an `open()` in between
            // cannot be missed.
            notified.as_mut().enable();
            if self.inner.is_open.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    pub fn open(&self) {
        self.inner.is_open.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }
}

pub struct WorkerGatedOrigin {
    pub payload: Bytes,
    pub gate: WorkerGate,
}

impl WorkerGatedOrigin {
    pub fn new(payload: impl Into<Bytes>, gate: WorkerGate) -> Self {
        Self {
            payload: payload.into(),
            gate,
        }
    }
}

#[async_trait]
impl HttpTransport for WorkerGatedOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let total = self.payload.len() as i64;
        let range = request.range.unwrap_or(ByteRange::new(0, total));
        let is_probe = range.start == 0 && range.end == 1;
        if is_probe {
            return Ok(RangeResponse {
                status_code: 206,
                total_size: Some(total),
                accepts_ranges: true,
                validator: Some("etag-gated".to_string()),
                resolved_url: request.url,
                body: Box::pin(futures::stream::empty()),
            });
        }

        let lower = range.start.min(total) as usize;
        let upper = range.end.min(total) as usize;
        let slice = self.payload.slice(lower..upper);
        let gate = self.gate.clone();
        let body = futures::stream::once(async move {
            gate.wait_until_open().await;
            Ok(slice)
        });
        Ok(RangeResponse {
            status_code: 206,
            total_size: Some(total),
            accepts_ranges: true,
            validator: Some("etag-gated".to_string()),
            resolved_url: request.url,
            body: Box::pin(body),
        })
    }
}
