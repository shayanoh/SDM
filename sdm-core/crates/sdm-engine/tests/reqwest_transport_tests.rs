mod support;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sdm_core::ByteRange;
use sdm_engine::{HttpTransport, RangeRequest, ReqwestTransport, RetryPolicy, TransportError};
use support::{
    LoopbackBody, LoopbackResponse, LoopbackServer, collect_body, collect_body_until_error,
    payload_handler, test_payload, test_source_url,
};

/// A transport that never consults the machine's proxy settings, so loopback
/// traffic cannot be routed through a configured proxy.
fn loopback_transport() -> ReqwestTransport {
    ReqwestTransport::with_builder(reqwest::Client::builder().no_proxy())
}

// `ReqwestTransport::fetch` is exercised below against a loopback server in
// this process (the one place a test may open a socket). The pure decisions
// at the HTTP boundary are also tested directly, over synthesized headers.

fn headers(fields: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in fields {
        map.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    map
}

// MARK: - Range header construction

#[test]
fn range_header_converts_half_open_to_inclusive() {
    assert_eq!(
        ReqwestTransport::range_header_value(Some(ByteRange::new(0, 1))).as_deref(),
        Some("bytes=0-0")
    );
    assert_eq!(
        ReqwestTransport::range_header_value(Some(ByteRange::new(100, 200))).as_deref(),
        Some("bytes=100-199")
    );
    // i64 offsets: a range past 4 GB must not be truncated to 32 bits.
    assert_eq!(
        ReqwestTransport::range_header_value(Some(ByteRange::new(5_000_000_000, 5_000_001_000)))
            .as_deref(),
        Some("bytes=5000000000-5000000999")
    );
}

#[test]
fn no_range_header_for_whole_resource_or_empty_range() {
    assert_eq!(ReqwestTransport::range_header_value(None), None);
    // An empty claim would otherwise become "bytes=10-9", which is malformed.
    assert_eq!(
        ReqwestTransport::range_header_value(Some(ByteRange::new(10, 10))),
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_carries_the_range_header_and_bypasses_the_cache() {
    support::within(async {
        // The origin invites caching and conditional requests; a second fetch
        // must still go to the wire unconditionally.
        let server = LoopbackServer::start(|_| {
            LoopbackResponse::ok(vec![1; 8])
                .header("Cache-Control", "max-age=3600")
                .header("ETag", "\"cached\"")
                .header("Last-Modified", "Mon, 03 Aug 2026 00:00:00 GMT")
        })
        .await;
        let transport = loopback_transport();
        let url = server.url("/file.bin");

        for _ in 0..2 {
            let request = RangeRequest::new(url.clone(), Some(ByteRange::new(8, 16)));
            let response = transport.fetch(request).await.unwrap();
            collect_body(response).await.unwrap();
        }
        let whole = transport
            .fetch(RangeRequest::new(url.clone(), None))
            .await
            .unwrap();
        collect_body(whole).await.unwrap();

        let seen = server.requests();
        assert_eq!(seen.len(), 3, "every fetch reaches the origin");
        for request in &seen[..2] {
            assert_eq!(request.target, "/file.bin");
            assert_eq!(request.header("range"), Some("bytes=8-15"));
        }
        assert_eq!(seen[2].header("range"), None);
        for request in &seen {
            assert_eq!(request.header("if-none-match"), None);
            assert_eq!(request.header("if-modified-since"), None);
        }
    })
    .await;
}

// MARK: - Total size selection

#[test]
fn content_range_total_wins_over_content_length() {
    let headers = headers(&[
        ("Content-Length", "1024"),
        ("Content-Range", "bytes 0-1023/1048576"),
    ]);
    assert_eq!(ReqwestTransport::total_size(&headers), Some(1_048_576));
}

#[test]
fn content_length_is_used_when_no_content_range_is_offered() {
    assert_eq!(
        ReqwestTransport::total_size(&headers(&[("Content-Length", "4096")])),
        Some(4096)
    );
}

#[test]
fn unknown_total_size_is_reported_as_none() {
    // No length at all — a chunked response.
    assert_eq!(ReqwestTransport::total_size(&headers(&[])), None);
    // A wildcard Content-Range total falls back to Content-Length, which is
    // also absent here.
    assert_eq!(
        ReqwestTransport::total_size(&headers(&[("Content-Range", "bytes 0-9/*")])),
        None
    );
}

#[test]
fn wildcard_content_range_falls_back_to_content_length() {
    let headers = headers(&[("Content-Range", "bytes 0-9/*"), ("Content-Length", "10")]);
    assert_eq!(ReqwestTransport::total_size(&headers), Some(10));
}

#[test]
fn multi_gigabyte_total_size_survives_as_i64() {
    let headers = headers(&[("Content-Range", "bytes 0-0/9663676416")]);
    assert_eq!(ReqwestTransport::total_size(&headers), Some(9_663_676_416));
}

// MARK: - Resumability and validator

#[test]
fn partial_content_implies_range_support() {
    assert!(ReqwestTransport::accepts_ranges(206, &headers(&[])));
}

#[test]
fn accept_ranges_header_implies_range_support_on_a_200() {
    assert!(ReqwestTransport::accepts_ranges(
        200,
        &headers(&[("Accept-Ranges", "bytes")])
    ));
    assert!(ReqwestTransport::accepts_ranges(
        200,
        &headers(&[("Accept-Ranges", "Bytes")])
    ));
    assert!(!ReqwestTransport::accepts_ranges(
        200,
        &headers(&[("Accept-Ranges", "none")])
    ));
    assert!(!ReqwestTransport::accepts_ranges(200, &headers(&[])));
}

#[test]
fn etag_is_preferred_over_last_modified_as_the_validator() {
    let both = headers(&[
        ("ETag", "\"abc\""),
        ("Last-Modified", "Mon, 03 Aug 2026 00:00:00 GMT"),
    ]);
    assert_eq!(
        ReqwestTransport::validator(&both).as_deref(),
        Some("\"abc\"")
    );

    let only_date = headers(&[("Last-Modified", "Mon, 03 Aug 2026 00:00:00 GMT")]);
    assert_eq!(
        ReqwestTransport::validator(&only_date).as_deref(),
        Some("Mon, 03 Aug 2026 00:00:00 GMT")
    );

    assert_eq!(ReqwestTransport::validator(&headers(&[])), None);
}

// MARK: - Client configuration

/// `URLSessionTransport.defaultConfiguration` pins the cache policy and a nil
/// `URLCache`. reqwest has no cache to configure; what that setting
/// guarantees is that a cacheable response is fetched again every time,
/// which is what this pins (the default-constructed transport, no override).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_configuration_never_touches_the_shared_cache() {
    support::within(async {
        let server = LoopbackServer::start(|_| {
            LoopbackResponse::ok(vec![7; 16]).header("Cache-Control", "public, max-age=86400")
        })
        .await;
        // The default transport, built the way the app builds it. The loopback
        // address is not proxied by any system configuration this suite runs
        // under; `request_carries_the_range_header_and_bypasses_the_cache` covers
        // the same ground with proxies disabled.
        let transport = ReqwestTransport::new();
        let url = server.url("/cacheable");
        for _ in 0..2 {
            let response = transport
                .fetch(RangeRequest::new(url.clone(), None))
                .await
                .unwrap();
            assert_eq!(collect_body(response).await.unwrap(), vec![7; 16]);
        }
        assert_eq!(server.requests().len(), 2);
    })
    .await;
}

// MARK: - Origin behavior through the real client (Review Focus 4)

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follows_redirects_and_reports_the_final_url() {
    support::within(async {
        let payload = test_payload(300);
        let body = payload.clone();
        let server = LoopbackServer::start(move |request| match request.target.as_str() {
            "/start" => LoopbackResponse::redirect(302, "/middle"),
            "/middle" => LoopbackResponse::redirect(307, "/backend/final.bin"),
            _ => payload_handler(body.clone(), true)(request),
        })
        .await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(
                server.url("/start"),
                Some(ByteRange::new(100, 200)),
            ))
            .await
            .unwrap();

        assert_eq!(response.status_code, 206);
        assert_eq!(response.resolved_url, server.url("/backend/final.bin"));
        assert_eq!(response.total_size, Some(300));
        assert_eq!(collect_body(response).await.unwrap(), payload[100..200]);
        // The Range header survives every hop, so the final backend slices.
        let seen = server.requests();
        assert_eq!(
            seen.iter().map(|r| r.target.as_str()).collect::<Vec<_>>(),
            ["/start", "/middle", "/backend/final.bin"]
        );
        assert!(
            seen.iter()
                .all(|r| r.header("range") == Some("bytes=100-199"))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_body_cut_short_surfaces_connection_dropped() {
    support::within(async {
        let payload = test_payload(1000);
        let sent = payload[..400].to_vec();
        let server = LoopbackServer::start(move |_| {
            LoopbackResponse::new(200)
                .header("Content-Length", "1000")
                .body(LoopbackBody::AbortedAfter(sent.clone()))
        })
        .await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(server.url("/cut"), None))
            .await
            .unwrap();
        assert_eq!(response.total_size, Some(1000));
        let (received, error) = collect_body_until_error(response).await;

        assert!(
            matches!(error, Some(TransportError::Network { .. })),
            "{error:?}"
        );
        // Whatever arrived before the cut is a faithful prefix.
        assert!(received.len() <= 400);
        assert_eq!(received, payload[..received.len()]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lying_content_length_is_reported_as_received() {
    support::within(async {
        let payload = test_payload(1000);
        let sent = payload.clone();
        let server = LoopbackServer::start(move |_| {
            LoopbackResponse::new(200)
                .header("Content-Length", "5000")
                .body(LoopbackBody::AbortedAfter(sent.clone()))
        })
        .await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(server.url("/lie"), None))
            .await
            .unwrap();
        // The header is taken at its word...
        assert_eq!(response.status_code, 200);
        assert_eq!(response.total_size, Some(5000));
        // ...and the shortfall shows up only when the body runs out.
        let (received, error) = collect_body_until_error(response).await;
        assert!(
            matches!(error, Some(TransportError::Network { .. })),
            "{error:?}"
        );
        assert_eq!(received, payload[..received.len()]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_origin_ignoring_range_reports_status_200_and_no_range_support() {
    support::within(async {
        let payload = test_payload(500);
        let server = LoopbackServer::start(payload_handler(payload.clone(), false)).await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(
                server.url("/whole"),
                Some(ByteRange::new(100, 200)),
            ))
            .await
            .unwrap();

        assert_eq!(response.status_code, 200);
        assert!(!response.accepts_ranges);
        assert_eq!(response.total_size, Some(500));
        assert_eq!(collect_body(response).await.unwrap(), payload);
        assert_eq!(server.requests()[0].header("range"), Some("bytes=100-199"));
    })
    .await;
}

// Not in the Swift suite: the rest of the HTTP boundary through a socket.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ranged_response_reports_206_content_range_and_the_validator() {
    support::within(async {
        let payload = test_payload(1000);
        let server = LoopbackServer::start(payload_handler(payload.clone(), true)).await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(
                server.url("/ranged"),
                Some(ByteRange::new(100, 200)),
            ))
            .await
            .unwrap();

        assert_eq!(response.status_code, 206);
        assert!(response.accepts_ranges);
        assert_eq!(response.total_size, Some(1000));
        assert_eq!(response.validator.as_deref(), Some("\"v1\""));
        assert_eq!(response.resolved_url, server.url("/ranged"));
        assert_eq!(collect_body(response).await.unwrap(), payload[100..200]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_status_is_returned_not_thrown() {
    support::within(async {
        // Like `URLSessionTransport`, the transport reports the status and leaves
        // the decision to the caller; only transport failures are errors.
        let server = LoopbackServer::start(|_| LoopbackResponse::new(403)).await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(test_url(&server, "/denied"), None))
            .await
            .unwrap();
        assert_eq!(response.status_code, 403);
    })
    .await;
}

fn test_url(server: &LoopbackServer, path: &str) -> url::Url {
    // Keeps the helper import list honest: the source URL is only a
    // `https://example.com` placeholder and never fetched here.
    assert_eq!(test_source_url().path(), "/file.bin");
    server.url(path)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_connection_names_its_cause_and_is_transient() {
    support::within(async {
        // Bind and release a port: nothing listens there any more.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url =
            url::Url::parse(&format!("http://{}/gone", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let transport = loopback_transport();

        let error = transport
            .fetch(RangeRequest::new(url, None))
            .await
            .expect_err("nothing is listening");
        assert!(matches!(error, TransportError::Network { .. }), "{error:?}");
        // The cause survives into the text the engine shows the user.
        let text = error.to_string().to_lowercase();
        assert!(text.contains("refused"), "cause missing from: {text}");
        assert_eq!(
            RetryPolicy::new().classify(&error),
            sdm_engine::FailureKind::Transient
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_body_stream_mid_body_closes_the_connection() {
    support::within(async {
        use futures::StreamExt;

        let server = LoopbackServer::start(|_| {
            LoopbackResponse::new(200)
                .header("Content-Length", "10000000000")
                .body(LoopbackBody::Endless {
                    prefix: b"first".to_vec(),
                    chunk_size: 16 * 1024,
                })
        })
        .await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(server.url("/endless"), None))
            .await
            .unwrap();
        let mut body = response.body;
        let first = body.next().await.expect("a chunk").unwrap();
        assert!(!first.is_empty());
        assert_eq!(
            server.abandoned_count(),
            0,
            "the body is still being served"
        );

        // The engine pauses a worker by dropping its stream.
        drop(body);

        server.wait_for_abandoned(1).await;
        assert_eq!(server.abandoned_count(), 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_fetch_before_the_response_head_cancels_the_request() {
    support::within(async {
        use tokio::io::AsyncReadExt;

        // A raw listener that reads the request and never answers; it can see
        // the client's side of the connection close, which a hyper service
        // parked on a pending future cannot report.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = url::Url::parse(&format!(
            "http://{}/stalled",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let transport = loopback_transport();

        // Boxed, not `tokio::pin!`ned: dropping a stack pin would drop only
        // the reference and leave the future alive.
        let mut fetch = Box::pin(transport.fetch(RangeRequest::new(url, None)));
        let (mut socket, _) = tokio::select! {
            _ = &mut fetch => panic!("a stalled origin must not answer"),
            accepted = listener.accept() => accepted.unwrap(),
        };
        let mut head = vec![0u8; 4096];
        // The client only writes its request while `fetch` is polled.
        let read = tokio::select! {
            _ = &mut fetch => panic!("a stalled origin must not answer"),
            read = socket.read(&mut head) => read.unwrap(),
        };
        assert!(String::from_utf8_lossy(&head[..read]).starts_with("GET /stalled"));

        // A cancelled worker gives up on the fetch...
        drop(fetch);

        // ...and the origin sees the connection close.
        let mut rest = Vec::new();
        assert_eq!(socket.read_to_end(&mut rest).await.unwrap(), 0);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cookie_set_on_a_redirect_hop_is_replayed_on_the_next_hop() {
    support::within(async {
        let server = LoopbackServer::start(move |request| match request.target.as_str() {
            "/start" => {
                LoopbackResponse::redirect(302, "/check").header("Set-Cookie", "sid=abc123; Path=/")
            }
            "/check"
                if request
                    .header("cookie")
                    .is_some_and(|c| c.contains("sid=abc123")) =>
            {
                LoopbackResponse::ok(b"welcome".to_vec())
            }
            _ => LoopbackResponse::new(403),
        })
        .await;
        let transport = loopback_transport();

        let response = transport
            .fetch(RangeRequest::new(server.url("/start"), None))
            .await
            .unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.resolved_url, server.url("/check"));
        assert_eq!(collect_body(response).await.unwrap(), b"welcome");

        // The jar lives as long as the transport: a later request carries it too.
        let later = transport
            .fetch(RangeRequest::new(server.url("/check"), None))
            .await
            .unwrap();
        assert_eq!(later.status_code, 200);
    })
    .await;
}
