use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use async_trait::async_trait;
use sdm_core::SourceUrl;
use tokio::sync::oneshot;

use crate::probe_transport::{
    ProbeError, ProbeMethod, ProbeRequest, ProbeResponse, ProbeTransport,
};

/// What the origin answers for one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeProbeOriginBehavior {
    pub status_code: i64,
    pub final_url: Option<SourceUrl>,
    /// Expected already-lowercased, matching what a real `ProbeTransport`
    /// hands back.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// A `Head` request fails with `MalformedResponse` — the trigger for
    /// stage 1's `Range: bytes=0-0` GET fallback.
    pub rejects_head: bool,
    pub error: Option<ProbeError>,
    /// Holds the request until [`FakeProbeOrigin::release`] is called for
    /// this URL, so concurrency-budget tests can observe an in-flight
    /// request.
    pub holds_until_released: bool,
}

impl Default for FakeProbeOriginBehavior {
    fn default() -> Self {
        Self {
            status_code: 200,
            final_url: None,
            headers: HashMap::new(),
            body: Vec::new(),
            rejects_head: false,
            error: None,
            holds_until_released: false,
        }
    }
}

#[derive(Default)]
struct State {
    behaviors: HashMap<SourceUrl, FakeProbeOriginBehavior>,
    request_log: Vec<ProbeRequest>,
    released: HashSet<SourceUrl>,
    release_waiters: HashMap<SourceUrl, Vec<oneshot::Sender<()>>>,
}

/// An in-process, per-URL programmable origin for grabber tests. See spec
/// §11.1 for the sibling `FakeOrigin` this mirrors in `sdm-engine`.
pub struct FakeProbeOrigin {
    state: Mutex<State>,
}

impl Default for FakeProbeOrigin {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeProbeOrigin {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
        }
    }

    pub fn set_behavior(&self, behavior: FakeProbeOriginBehavior, url: &SourceUrl) {
        self.state
            .lock()
            .unwrap()
            .behaviors
            .insert(url.clone(), behavior);
    }

    pub fn release(&self, url: &SourceUrl) {
        let waiters = {
            let mut state = self.state.lock().unwrap();
            state.released.insert(url.clone());
            state.release_waiters.remove(url).unwrap_or_default()
        };
        for waiter in waiters {
            let _ = waiter.send(());
        }
    }

    pub fn request_log(&self) -> Vec<ProbeRequest> {
        self.state.lock().unwrap().request_log.clone()
    }
}

#[async_trait]
impl ProbeTransport for FakeProbeOrigin {
    async fn send(&self, request: ProbeRequest) -> Result<ProbeResponse, ProbeError> {
        // The default for an unconfigured URL is a 404.
        let (behavior, hold) = {
            let mut state = self.state.lock().unwrap();
            state.request_log.push(request.clone());
            let behavior = state
                .behaviors
                .get(&request.url)
                .cloned()
                .unwrap_or_else(|| FakeProbeOriginBehavior {
                    status_code: 404,
                    ..FakeProbeOriginBehavior::default()
                });
            // Checking `released` and registering the waiter under one lock
            // means a `release` cannot slip between them.
            let hold = if behavior.holds_until_released && !state.released.contains(&request.url) {
                let (sender, receiver) = oneshot::channel();
                state
                    .release_waiters
                    .entry(request.url.clone())
                    .or_default()
                    .push(sender);
                Some(receiver)
            } else {
                None
            };
            (behavior, hold)
        };
        if let Some(receiver) = hold {
            let _ = receiver.await;
        }

        if let Some(error) = behavior.error {
            return Err(error);
        }
        if request.method == ProbeMethod::Head && behavior.rejects_head {
            return Err(ProbeError::MalformedResponse);
        }

        let body = match (request.method, request.range) {
            (ProbeMethod::Head, _) => Vec::new(),
            (ProbeMethod::Get, Some(range)) => {
                let count = behavior.body.len() as i64;
                let lower = range.start.min(count) as usize;
                let upper = range.end.min(count) as usize;
                behavior.body[lower..upper].to_vec()
            }
            (ProbeMethod::Get, None) => behavior.body.clone(),
        };

        Ok(ProbeResponse {
            status_code: behavior.status_code,
            final_url: behavior.final_url.unwrap_or(request.url),
            headers: behavior.headers,
            body,
        })
    }
}
