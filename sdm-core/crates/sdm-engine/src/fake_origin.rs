use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use sdm_core::ByteRange;

use crate::http_transport::{HttpTransport, RangeRequest, RangeResponse, TransportError};

/// How a [`FakeOrigin`] misbehaves. Swift's `FakeOrigin.Behavior`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeOriginBehavior {
    /// Serve the whole body regardless of the requested range.
    pub ignores_ranges: bool,
    /// Report this size instead of the true payload size.
    pub reported_size_override: Option<i64>,
    /// Fail with `ConnectionDropped` after emitting this many bytes.
    pub drop_after_bytes: Option<usize>,
    /// End the body stream cleanly (no error) after emitting this many
    /// bytes — simulates a server that closes the connection without
    /// signaling failure, short of the requested range.
    pub truncate_after_bytes: Option<usize>,
    /// Return this status instead of 200/206.
    pub status_override: Option<u16>,
    /// Value reported as the `ETag`.
    pub validator: Option<String>,
    /// Bytes emitted per chunk of the response stream.
    pub chunk_size: usize,
}

impl Default for FakeOriginBehavior {
    fn default() -> Self {
        Self {
            ignores_ranges: false,
            reported_size_override: None,
            drop_after_bytes: None,
            truncate_after_bytes: None,
            status_override: None,
            validator: None,
            chunk_size: 64,
        }
    }
}

struct State {
    behavior: FakeOriginBehavior,
    requested_ranges: Vec<ByteRange>,
}

/// An in-process origin server for tests, programmable to misbehave the way
/// real servers do. See spec §11.1. Cloning shares the same origin.
#[derive(Clone)]
pub struct FakeOrigin {
    payload: Bytes,
    state: Arc<Mutex<State>>,
}

impl FakeOrigin {
    pub fn new(payload: impl Into<Bytes>) -> Self {
        Self::with_behavior(payload, FakeOriginBehavior::default())
    }

    pub fn with_behavior(payload: impl Into<Bytes>, behavior: FakeOriginBehavior) -> Self {
        Self {
            payload: payload.into(),
            state: Arc::new(Mutex::new(State {
                behavior,
                requested_ranges: Vec::new(),
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_validator(&self, validator: Option<String>) {
        self.lock().behavior.validator = validator;
    }

    pub fn set_behavior(&self, behavior: FakeOriginBehavior) {
        self.lock().behavior = behavior;
    }

    pub fn requested_ranges(&self) -> Vec<ByteRange> {
        self.lock().requested_ranges.clone()
    }
}

#[async_trait]
impl HttpTransport for FakeOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let total = self.payload.len() as i64;
        let behavior = {
            let mut state = self.lock();
            if let Some(range) = request.range {
                state.requested_ranges.push(range);
            }
            state.behavior.clone()
        };

        if let Some(status) = behavior.status_override {
            return Ok(RangeResponse {
                status_code: status,
                total_size: Some(behavior.reported_size_override.unwrap_or(total)),
                accepts_ranges: !behavior.ignores_ranges,
                validator: behavior.validator,
                resolved_url: request.url,
                body: Box::pin(futures::stream::empty()),
            });
        }

        let (slice, status) = match request.range {
            Some(range) if !behavior.ignores_ranges => {
                let lower = range.start.min(total) as usize;
                let upper = range.end.min(total) as usize;
                (self.payload.slice(lower..upper), 206)
            }
            _ => (self.payload.clone(), 200),
        };

        // Built eagerly, like the Swift stream whose closure runs to
        // completion at creation: every chunk, then the cut-off.
        let chunk_size = behavior.chunk_size.max(1);
        let mut items: Vec<Result<Bytes, TransportError>> = Vec::new();
        let mut emitted = 0usize;
        let mut offset = 0usize;
        while offset < slice.len() {
            let end = (offset + chunk_size).min(slice.len());
            if let Some(limit) = behavior.drop_after_bytes
                && emitted + (end - offset) > limit
            {
                let allowed = limit - emitted;
                if allowed > 0 {
                    items.push(Ok(slice.slice(offset..offset + allowed)));
                }
                items.push(Err(TransportError::ConnectionDropped));
                break;
            }
            if let Some(limit) = behavior.truncate_after_bytes
                && emitted + (end - offset) > limit
            {
                let allowed = limit - emitted;
                if allowed > 0 {
                    items.push(Ok(slice.slice(offset..offset + allowed)));
                }
                break;
            }
            items.push(Ok(slice.slice(offset..end)));
            emitted += end - offset;
            offset = end;
        }

        Ok(RangeResponse {
            status_code: status,
            total_size: Some(behavior.reported_size_override.unwrap_or(total)),
            accepts_ranges: !behavior.ignores_ranges,
            validator: behavior.validator,
            resolved_url: request.url,
            body: Box::pin(futures::stream::iter(items)),
        })
    }
}
