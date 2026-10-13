use std::time::Duration;

use sdm_resolve::testing::FakeBinaryFetcher;
use sdm_resolve::{BinaryFetchError, BinaryFetching, ReqwestBinaryFetcher};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_fetcher_replays_and_throws() {
    let f = FakeBinaryFetcher::new();
    f.set_response(Ok(b"hi".to_vec()), &url("https://x/a"));
    f.set_response(
        Err(BinaryFetchError::Transport("offline".into())),
        &url("https://x/b"),
    );

    assert_eq!(f.data(&url("https://x/a")).await.unwrap(), b"hi".to_vec());
    assert_eq!(
        f.data(&url("https://x/b")).await,
        Err(BinaryFetchError::Transport("offline".into()))
    );
    assert_eq!(f.request_count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_fetcher_unknown_url_throws404() {
    let f = FakeBinaryFetcher::new();
    assert_eq!(
        f.data(&url("https://x/missing")).await,
        Err(BinaryFetchError::Http(404))
    );
}

// The Swift suite has no test for `URLSessionBinaryFetcher`; the tests below
// pin `ReqwestBinaryFetcher` against a loopback server in this process.

/// Serves each connection with `respond(request_head)` and records request
/// heads. Returns the base URL.
async fn serve(
    respond: impl Fn(&str) -> Vec<u8> + Send + Sync + 'static,
) -> (Url, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = url(&format!("http://{}/", listener.local_addr().unwrap()));
    let heads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = heads.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let reply = respond(&head);
            recorded.lock().unwrap().push(head);
            let _ = socket.write_all(&reply).await;
            let _ = socket.shutdown().await;
        }
    });
    (base, heads)
}

fn http(status: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// A fetcher that never consults the machine's proxy settings.
fn loopback_fetcher() -> ReqwestBinaryFetcher {
    ReqwestBinaryFetcher::with_builder(reqwest::Client::builder().no_proxy())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reqwest_fetcher_returns_the_body_and_sends_the_sdm_user_agent() {
    let (base, heads) = serve(|_| http("200 OK", "", b"payload")).await;
    let body = loopback_fetcher().data(&base).await.unwrap();
    assert_eq!(body, b"payload".to_vec());
    let head = heads.lock().unwrap()[0].to_ascii_lowercase();
    assert!(head.contains("user-agent: sdm\r\n"), "{head}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reqwest_fetcher_maps_non_2xx_to_http_error() {
    let (base, _) = serve(|_| http("404 Not Found", "", b"nope")).await;
    assert_eq!(
        loopback_fetcher().data(&base).await,
        Err(BinaryFetchError::Http(404))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reqwest_fetcher_follows_redirects() {
    let (base, _) = serve(|head| {
        if head.starts_with("GET /final") {
            http("200 OK", "", b"landed")
        } else {
            http("302 Found", "Location: /final\r\n", b"")
        }
    })
    .await;
    assert_eq!(
        loopback_fetcher().data(&base).await.unwrap(),
        b"landed".to_vec()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reqwest_fetcher_maps_connection_failure_to_transport_error() {
    // Bind then drop to get a port nothing listens on.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        loopback_fetcher().data(&url(&format!("http://127.0.0.1:{port}/"))),
    )
    .await
    .expect("connection refusal should be immediate");
    assert!(matches!(result, Err(BinaryFetchError::Transport(_))));
}
