mod support;

use sdm_core::ByteRange;
use sdm_grabber::{
    FakeProbeOrigin, FakeProbeOriginBehavior, ProbeError, ProbeMethod, ProbeRequest, ProbeTransport,
};
use support::{su, within};

const URL: &str = "https://example.com/file.zip";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn head_request_returns_configured_headers() {
    within(async {
        let url = su(URL);
        let origin = FakeProbeOrigin::new();
        let behavior = FakeProbeOriginBehavior {
            status_code: 200,
            headers: [
                ("content-length".to_string(), "1000".to_string()),
                ("content-type".to_string(), "application/zip".to_string()),
            ]
            .into(),
            ..FakeProbeOriginBehavior::default()
        };
        origin.set_behavior(behavior, &url);

        let response = origin
            .send(ProbeRequest::new(url, ProbeMethod::Head))
            .await
            .unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.headers.get("content-length").map(String::as_str),
            Some("1000")
        );
        assert!(response.body.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_request_with_range_returns_slice() {
    within(async {
        let url = su(URL);
        let origin = FakeProbeOrigin::new();
        let behavior = FakeProbeOriginBehavior {
            body: (0..100u8).collect(),
            ..FakeProbeOriginBehavior::default()
        };
        origin.set_behavior(behavior, &url);

        let response = origin
            .send(ProbeRequest::new(url, ProbeMethod::Get).with_range(ByteRange::new(10, 20)))
            .await
            .unwrap();
        assert_eq!(response.body, (10..20u8).collect::<Vec<u8>>());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_head_throws_malformed_response() {
    within(async {
        let url = su(URL);
        let origin = FakeProbeOrigin::new();
        let behavior = FakeProbeOriginBehavior {
            rejects_head: true,
            ..FakeProbeOriginBehavior::default()
        };
        origin.set_behavior(behavior, &url);

        let result = origin.send(ProbeRequest::new(url, ProbeMethod::Head)).await;
        assert_eq!(result.unwrap_err(), ProbeError::MalformedResponse);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_behavior_is_404_for_unconfigured_url() {
    within(async {
        let origin = FakeProbeOrigin::new();
        let response = origin
            .send(ProbeRequest::new(su(URL), ProbeMethod::Head))
            .await
            .unwrap();
        assert_eq!(response.status_code, 404);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_log_records_each_request_in_order() {
    within(async {
        let url = su(URL);
        let origin = FakeProbeOrigin::new();
        origin
            .send(ProbeRequest::new(url.clone(), ProbeMethod::Head))
            .await
            .unwrap();
        origin
            .send(ProbeRequest::new(url, ProbeMethod::Get).with_range(ByteRange::new(0, 1)))
            .await
            .unwrap();
        let methods: Vec<ProbeMethod> = origin.request_log().iter().map(|r| r.method).collect();
        assert_eq!(methods, vec![ProbeMethod::Head, ProbeMethod::Get]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn holds_until_released_blocks_until_release_called() {
    within(async {
        let url = su(URL);
        let origin = std::sync::Arc::new(FakeProbeOrigin::new());
        let behavior = FakeProbeOriginBehavior {
            holds_until_released: true,
            ..FakeProbeOriginBehavior::default()
        };
        origin.set_behavior(behavior, &url);

        let task = tokio::spawn({
            let origin = origin.clone();
            let url = url.clone();
            async move { origin.send(ProbeRequest::new(url, ProbeMethod::Head)).await }
        });
        while origin.request_log().is_empty() {
            tokio::task::yield_now().await;
        }
        // Still holding: the send call has registered but not returned.
        assert!(!task.is_finished());
        origin.release(&url);
        task.await.unwrap().unwrap();
    })
    .await;
}
