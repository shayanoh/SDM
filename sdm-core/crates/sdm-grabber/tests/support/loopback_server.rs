//! An HTTP/1.1 server on `127.0.0.1:0` inside the test process, for the
//! transport tests only (the one place the suite may open a socket).
//!
//! The handler is a closure from the parsed request to a [`LoopbackResponse`].
//! Responses can redirect, declare a `Content-Length` that disagrees with the
//! bytes sent, cut the connection mid-body, or stream forever; the server
//! counts the endless bodies hyper drops so a test can prove a client
//! disconnect reached it.

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::stream;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

/// What the server saw of one request.
#[derive(Debug, Clone)]
pub struct LoopbackRequest {
    pub method: String,
    /// Path and query, as sent.
    pub target: String,
    pub headers: HeaderMap,
}

impl LoopbackRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The `Range: bytes=a-b` header as `(a, Some(b))` (inclusive), or
    /// `(a, None)` for an open-ended `bytes=a-`.
    pub fn byte_range(&self) -> Option<(i64, Option<i64>)> {
        let spec = self.header("range")?.strip_prefix("bytes=")?;
        let (start, end) = spec.split_once('-')?;
        let start = start.parse().ok()?;
        let end = if end.is_empty() {
            None
        } else {
            Some(end.parse().ok()?)
        };
        Some((start, end))
    }
}

/// What the server sends for the body.
#[derive(Debug, Clone)]
pub enum LoopbackBody {
    /// These bytes; hyper sets `Content-Length` from them.
    Bytes(Vec<u8>),
    /// These bytes, then the connection is aborted. Pair with an explicit
    /// `Content-Length` header: a lying length, or a body cut short.
    AbortedAfter(Vec<u8>),
    /// `prefix`, then zero-filled chunks without end, until the client
    /// disconnects. Each drop of such a body is counted.
    Endless { prefix: Vec<u8>, chunk_size: usize },
}

#[derive(Debug, Clone)]
pub struct LoopbackResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: LoopbackBody,
}

impl LoopbackResponse {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: LoopbackBody::Bytes(Vec::new()),
        }
    }

    pub fn ok(body: Vec<u8>) -> Self {
        Self::new(200).body(LoopbackBody::Bytes(body))
    }

    pub fn redirect(status: u16, location: &str) -> Self {
        Self::new(status).header("Location", location)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn body(mut self, body: LoopbackBody) -> Self {
        self.body = body;
        self
    }
}

/// A handler that serves `payload` like an origin: with `honors_ranges` it
/// answers `Range` with `206` and `Content-Range` and advertises
/// `Accept-Ranges: bytes`; without, it ignores `Range` and answers `200`
/// with the whole body and no range advertisement.
pub fn payload_handler(
    payload: Vec<u8>,
    honors_ranges: bool,
) -> impl Fn(&LoopbackRequest) -> LoopbackResponse + Send + Sync + 'static {
    move |request| {
        let total = payload.len() as i64;
        match request.byte_range() {
            Some((start, end)) if honors_ranges => {
                let start = start.min(total);
                let end = end.map_or(total, |end| (end + 1).min(total));
                LoopbackResponse::new(206)
                    .header("Accept-Ranges", "bytes")
                    .header("ETag", "\"v1\"")
                    .header(
                        "Content-Range",
                        &format!("bytes {start}-{}/{total}", end - 1),
                    )
                    .body(LoopbackBody::Bytes(
                        payload[start as usize..end as usize].to_vec(),
                    ))
            }
            _ => {
                let response = LoopbackResponse::ok(payload.clone()).header("ETag", "\"v1\"");
                if honors_ranges {
                    response.header("Accept-Ranges", "bytes")
                } else {
                    response
                }
            }
        }
    }
}

type Handler = dyn Fn(&LoopbackRequest) -> LoopbackResponse + Send + Sync;

struct Shared {
    requests: Mutex<Vec<LoopbackRequest>>,
    abandoned: AtomicUsize,
    abandoned_notify: Notify,
}

/// Counts its own drop; held by the body or request future it belongs to.
struct AbandonGuard(Arc<Shared>);

impl Drop for AbandonGuard {
    fn drop(&mut self) {
        self.0.abandoned.fetch_add(1, Ordering::SeqCst);
        // `notify_one` stores a permit, so a waiter that has not parked yet
        // still wakes.
        self.0.abandoned_notify.notify_one();
    }
}

pub struct LoopbackServer {
    address: SocketAddr,
    shared: Arc<Shared>,
    shutdown: CancellationToken,
}

impl LoopbackServer {
    pub async fn start(
        handler: impl Fn(&LoopbackRequest) -> LoopbackResponse + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let address = listener.local_addr().expect("loopback address");
        let shared = Arc::new(Shared {
            requests: Mutex::new(Vec::new()),
            abandoned: AtomicUsize::new(0),
            abandoned_notify: Notify::new(),
        });
        let shutdown = CancellationToken::new();
        let handler: Arc<Handler> = Arc::new(handler);

        let accept_shared = shared.clone();
        let accept_shutdown = shutdown.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    () = accept_shutdown.cancelled() => return,
                };
                let Ok((socket, _)) = accepted else { continue };
                let shared = accept_shared.clone();
                let handler = handler.clone();
                let shutdown = accept_shutdown.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        respond(shared.clone(), handler.clone(), request)
                    });
                    let connection = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service);
                    tokio::select! {
                        _ = connection => {}
                        () = shutdown.cancelled() => {}
                    }
                });
            }
        });

        Self {
            address,
            shared,
            shutdown,
        }
    }

    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://{}{path}", self.address)).expect("loopback URL")
    }

    pub fn requests(&self) -> Vec<LoopbackRequest> {
        self.shared.requests.lock().unwrap().clone()
    }

    /// Endless bodies hyper has dropped so far, which
    /// happens when the client closes the connection.
    pub fn abandoned_count(&self) -> usize {
        self.shared.abandoned.load(Ordering::SeqCst)
    }

    /// Resolves once `count` endless bodies have been dropped. The server notices a disconnect by itself, with no sleeping;
    /// a test that never reaches `count` fails
    /// after `TEST_TIMEOUT` instead of hanging.
    pub async fn wait_for_abandoned(&self, count: usize) {
        super::within(async {
            while self.abandoned_count() < count {
                self.shared.abandoned_notify.notified().await;
            }
        })
        .await;
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

type ResponseBody = BoxBody<Bytes, io::Error>;

async fn respond(
    shared: Arc<Shared>,
    handler: Arc<Handler>,
    request: Request<hyper::body::Incoming>,
) -> Result<Response<ResponseBody>, Infallible> {
    let recorded = LoopbackRequest {
        method: request.method().to_string(),
        target: request
            .uri()
            .path_and_query()
            .map_or_else(|| "/".to_string(), ToString::to_string),
        headers: request.headers().clone(),
    };
    shared.requests.lock().unwrap().push(recorded.clone());
    let planned = handler(&recorded);

    let body: ResponseBody = match planned.body {
        LoopbackBody::Bytes(bytes) => Full::new(Bytes::from(bytes))
            .map_err(|never| match never {})
            .boxed(),
        LoopbackBody::AbortedAfter(bytes) => {
            // The `yield_now` lets hyper flush the data frame before the
            // error tears the connection down.
            let frames = stream::unfold(0u8, move |step| {
                let bytes = bytes.clone();
                async move {
                    match step {
                        0 => Some((Ok(Frame::data(Bytes::from(bytes))), 1)),
                        1 => {
                            tokio::task::yield_now().await;
                            Some((Err(io::Error::from(io::ErrorKind::ConnectionAborted)), 2))
                        }
                        _ => None,
                    }
                }
            });
            StreamBody::new(frames).boxed()
        }
        LoopbackBody::Endless { prefix, chunk_size } => {
            let guard = AbandonGuard(shared.clone());
            let chunk = Bytes::from(vec![0u8; chunk_size]);
            let frames = stream::unfold(
                (guard, Some(Bytes::from(prefix)), chunk),
                |(guard, prefix, chunk)| async move {
                    tokio::task::yield_now().await;
                    let next = prefix.unwrap_or_else(|| chunk.clone());
                    Some((Ok(Frame::data(next)), (guard, None, chunk)))
                },
            );
            StreamBody::new(frames).boxed()
        }
    };

    let mut response = Response::builder().status(planned.status);
    for (name, value) in &planned.headers {
        response = response.header(name, value);
    }
    Ok(response.body(body).expect("a valid loopback response"))
}
