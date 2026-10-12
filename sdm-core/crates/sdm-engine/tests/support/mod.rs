//! Shared helpers for the engine's integration tests. Each test file pulls
//! this in with `mod support;`, so any one file uses only some of it; the
//! blanket `allow`s below keep the rest from warning there.
#![allow(dead_code, unused_imports)]

mod engine_support;
mod fake_muxer;
mod fake_resolver;
mod fake_wholesale_downloader;
mod loopback_server;
mod routing_transport;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use sdm_core::{ByteRange, SourceUrl};
use sdm_engine::{
    DownloadTaskConfiguration, FakeOrigin, HttpTransport, RangeRequest, RangeResponse,
    TransportError,
};
use tokio::sync::Notify;
use url::Url;

pub use engine_support::{
    Gate, GatePhase, GatedOrigin, ScratchTrash, engine_settings, example_item, make_gated_engine,
    new_engine, new_engine_with_trash, package_of, running_count, snapshot_item, wait_for_progress,
    wait_until_async,
};
pub use fake_muxer::FakeMuxer;
pub use fake_resolver::FakeResolver;
pub use fake_wholesale_downloader::FakeWholesaleDownloader;
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

pub fn test_source() -> SourceUrl {
    SourceUrl::new("https://example.com/file.bin").expect("a valid URL")
}

/// Swift's `DownloadTask.Configuration.test(workers:minChunk:)`.
pub fn test_configuration(workers: usize, min_chunk: i64) -> DownloadTaskConfiguration {
    DownloadTaskConfiguration::new(workers, min_chunk, 128, None)
}

/// Waits (yielding, never sleeping) until `condition` holds; fails the test
/// instead of hanging if it never does.
pub async fn wait_until(mut condition: impl FnMut() -> bool) {
    within(async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
}

/// Swift's `SeededGenerator` (an xorshift), so a failing case is reproducible
/// from its seed.
pub struct SeededGenerator {
    state: u64,
}

impl SeededGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// A value in `low..=high`.
    pub fn in_range(&mut self, low: u64, high: u64) -> u64 {
        assert!(low <= high);
        low + self.next_u64() % (high - low + 1)
    }
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
    requested: Mutex<Vec<ByteRange>>,
}

impl WorkerGatedOrigin {
    pub fn new(payload: impl Into<Bytes>, gate: WorkerGate) -> Self {
        Self {
            payload: payload.into(),
            gate,
            requested: Mutex::new(Vec::new()),
        }
    }

    /// Every ranged request the origin has seen, in order (like
    /// `FakeOrigin::requested_ranges`).
    pub fn requested_ranges(&self) -> Vec<ByteRange> {
        self.requested.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpTransport for WorkerGatedOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let total = self.payload.len() as i64;
        let range = request.range.unwrap_or(ByteRange::new(0, total));
        self.requested.lock().unwrap().push(range);
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

/// Wraps a `FakeOrigin` so each non-probe body delivers `free_items` chunks
/// and then stalls until `gate` opens, leaving a download deterministically
/// part-way through: some bytes written, the rest still in flight.
pub struct StallingOrigin {
    pub inner: FakeOrigin,
    pub gate: WorkerGate,
    pub free_items: usize,
}

#[async_trait]
impl HttpTransport for StallingOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let is_probe = request.range.is_some_and(|range| range.length() == 1);
        let mut response = self.inner.fetch(request).await?;
        if !is_probe {
            let gate = self.gate.clone();
            let free_items = self.free_items;
            response.body = Box::pin(response.body.enumerate().then(move |(index, item)| {
                let gate = gate.clone();
                async move {
                    if index >= free_items {
                        gate.wait_until_open().await;
                    }
                    item
                }
            }));
        }
        Ok(response)
    }
}
