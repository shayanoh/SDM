mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Name, Resolve, Resolving};
use sdm_core::{ByteRange, SourceUrl};
use sdm_grabber::{ProbeError, ProbeMethod, ProbeRequest, ProbeTransport, ReqwestProbeTransport};
use support::{LoopbackBody, LoopbackResponse, LoopbackServer, su, within};
use tokio::net::TcpListener;

/// A transport that never consults the machine's proxy settings, so loopback
/// traffic cannot be routed through a configured proxy.
fn loopback_transport() -> ReqwestProbeTransport {
    ReqwestProbeTransport::with_builder(reqwest::Client::builder().no_proxy())
}

fn source(server: &LoopbackServer, path: &str) -> SourceUrl {
    SourceUrl::from_url(&server.url(path))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_uses_head_method_with_no_range_header() {
    within(async {
        let server = LoopbackServer::start(|_| LoopbackResponse::new(200)).await;
        let response = loopback_transport()
            .send(ProbeRequest::new(source(&server, "/f"), ProbeMethod::Head))
            .await
            .unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "HEAD");
        assert_eq!(seen[0].header("range"), None);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_uses_get_method_with_range_header_when_provided() {
    within(async {
        let server = LoopbackServer::start(|_| LoopbackResponse::new(206)).await;
        loopback_transport()
            .send(
                ProbeRequest::new(source(&server, "/f"), ProbeMethod::Get)
                    .with_range(ByteRange::new(0, 65536)),
            )
            .await
            .unwrap();

        let seen = server.requests();
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].header("range"), Some("bytes=0-65535"));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_carries_no_range_header_for_nil_range() {
    within(async {
        let server = LoopbackServer::start(|_| LoopbackResponse::new(200)).await;
        loopback_transport()
            .send(ProbeRequest::new(source(&server, "/f"), ProbeMethod::Get))
            .await
            .unwrap();
        assert_eq!(server.requests()[0].header("range"), None);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn headers_are_lowercased_for_case_insensitive_lookup() {
    within(async {
        let server = LoopbackServer::start(|_| {
            LoopbackResponse::new(200)
                .header("Content-Type", "application/zip")
                .header("ETag", "\"abc\"")
        })
        .await;
        let response = loopback_transport()
            .send(ProbeRequest::new(source(&server, "/f"), ProbeMethod::Head))
            .await
            .unwrap();
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("application/zip")
        );
        assert_eq!(
            response.headers.get("etag").map(String::as_str),
            Some("\"abc\"")
        );
        assert!(response.headers.keys().all(|k| *k == k.to_lowercase()));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_header_with_raw_utf8_bytes_is_kept_not_dropped() {
    within(async {
        // Servers commonly send a raw (unencoded) UTF-8 filename.
        let server = LoopbackServer::start(|_| {
            LoopbackResponse::new(200).header(
                "Content-Disposition",
                "attachment; filename=\"سلام café.zip\"",
            )
        })
        .await;
        let response = loopback_transport()
            .send(ProbeRequest::new(source(&server, "/f"), ProbeMethod::Head))
            .await
            .unwrap();
        assert_eq!(
            response
                .headers
                .get("content-disposition")
                .map(String::as_str),
            Some("attachment; filename=\"سلام café.zip\"")
        );
    })
    .await;
}

#[test]
fn a_header_with_invalid_utf8_bytes_is_decoded_as_latin1() {
    use reqwest::header::{CONTENT_DISPOSITION, HeaderMap, HeaderValue};
    let mut map = HeaderMap::new();
    // "caf\xE9" is Latin-1 "café" and not valid UTF-8.
    map.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_bytes(b"attachment; filename=\"caf\xE9.zip\"").unwrap(),
    );
    let lowered = ReqwestProbeTransport::lowercased_headers(&map);
    assert_eq!(
        lowered.get("content-disposition").map(String::as_str),
        Some("attachment; filename=\"café.zip\"")
    );
}

/// A resolver that always fails, so a DNS failure is real reqwest behaviour
/// without a lookup leaving the machine.
struct FailingResolver;

impl Resolve for FailingResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        Box::pin(async {
            Err(Box::new(std::io::Error::other("no such host"))
                as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn classify_maps_timeout_and_dns_error_codes() {
    within(async {
        // Timeout: an origin that accepts the connection and never answers.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        let hold = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let transport = ReqwestProbeTransport::with_builder_and_idle_timeout(
            reqwest::Client::builder().no_proxy(),
            Duration::from_millis(150),
        );
        let url = su(&format!("http://{address}/f"));
        let error = transport
            .send(ProbeRequest::new(url, ProbeMethod::Head))
            .await
            .unwrap_err();
        assert_eq!(error, ProbeError::TimedOut);
        // Dropping a `JoinHandle` only detaches the accept task; it keeps
        // running (and holding its sockets) until the runtime shuts down when
        // the test ends. Nothing here depends on it stopping sooner.
        drop(hold);

        // DNS: the resolver fails.
        let transport = ReqwestProbeTransport::with_builder(
            reqwest::Client::builder()
                .no_proxy()
                .dns_resolver(Arc::new(FailingResolver)),
        );
        let error = transport
            .send(ProbeRequest::new(
                su("http://nonexistent.test/f"),
                ProbeMethod::Head,
            ))
            .await
            .unwrap_err();
        assert_eq!(error, ProbeError::DnsFailure);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn classify_returns_nil_for_unrecognized_error_codes() {
    within(async {
        // A port nothing listens on: refused, which is neither a timeout nor
        // a DNS failure, so it keeps its cause as a generic network error.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let error = loopback_transport()
            .send(ProbeRequest::new(
                su(&format!("http://{address}/f")),
                ProbeMethod::Head,
            ))
            .await
            .unwrap_err();
        let ProbeError::Network { description } = error else {
            panic!("expected a network error, got {error:?}");
        };
        assert!(!description.is_empty());
    })
    .await;
}

// Rust-only: behaviour of the Swift `send` that the six ported tests above
// do not reach (they only cover its static helpers).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_stops_reading_at_the_requested_range_size_and_reports_the_final_url() {
    within(async {
        let payload: Vec<u8> = (0..200u8).collect();
        let server = LoopbackServer::start(move |request| match request.target.as_str() {
            "/start" => LoopbackResponse::redirect(302, "/final"),
            // An origin that ignores `Range` and sends everything.
            _ => LoopbackResponse::new(200).body(LoopbackBody::Bytes(payload.clone())),
        })
        .await;
        let transport = loopback_transport();

        let sliced = transport
            .send(
                ProbeRequest::new(source(&server, "/start"), ProbeMethod::Get)
                    .with_range(ByteRange::new(0, 16)),
            )
            .await
            .unwrap();
        assert_eq!(sliced.body, (0..16u8).collect::<Vec<u8>>());
        assert_eq!(sliced.final_url, source(&server, "/final"));

        // No range: the Swift loop appends one byte and then stops.
        let bare = transport
            .send(ProbeRequest::new(
                source(&server, "/final"),
                ProbeMethod::Get,
            ))
            .await
            .unwrap();
        assert_eq!(bare.body, vec![0u8]);
    })
    .await;
}
