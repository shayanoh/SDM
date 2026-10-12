mod support;

use sdm_core::ByteRange;
use sdm_engine::{FakeOrigin, FakeOriginBehavior, HttpTransport, RangeRequest, TransportError};
use support::collect_body;
use url::Url;

fn url() -> Url {
    Url::parse("https://example.com/file.bin").unwrap()
}

fn payload(count: usize) -> Vec<u8> {
    (0..count).map(|i| (i % 251) as u8).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_fetch_returns_whole_payload() {
    let origin = FakeOrigin::new(payload(1000));
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    assert_eq!(response.total_size, Some(1000));
    assert!(response.accepts_ranges);
    assert_eq!(collect_body(response).await.unwrap(), payload(1000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ranged_fetch_returns_only_that_slice() {
    let origin = FakeOrigin::new(payload(1000));
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: Some(ByteRange::new(100, 200)),
        })
        .await
        .unwrap();
    assert_eq!(response.status_code, 206);
    assert_eq!(
        collect_body(response).await.unwrap(),
        payload(1000)[100..200]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_ignoring_ranges_returns_whole_body() {
    let behavior = FakeOriginBehavior {
        ignores_ranges: true,
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: Some(ByteRange::new(100, 200)),
        })
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    assert!(!response.accepts_ranges);
    assert_eq!(collect_body(response).await.unwrap().len(), 1000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_drops_connection_at_configured_offset() {
    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(40),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(
        collect_body(response).await,
        Err(TransportError::ConnectionDropped)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_can_report_wrong_content_length() {
    let behavior = FakeOriginBehavior {
        reported_size_override: Some(5000),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(response.total_size, Some(5000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_can_return_error_status() {
    let behavior = FakeOriginBehavior {
        status_override: Some(403),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let response = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(response.status_code, 403);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_validator_can_change_between_requests() {
    let behavior = FakeOriginBehavior {
        validator: Some("etag-1".to_string()),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let first = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(first.validator.as_deref(), Some("etag-1"));
    origin.set_validator(Some("etag-2".to_string()));
    let second = origin
        .fetch(RangeRequest {
            url: url(),
            range: None,
        })
        .await
        .unwrap();
    assert_eq!(second.validator.as_deref(), Some("etag-2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_records_requested_ranges() {
    let origin = FakeOrigin::new(payload(1000));
    origin
        .fetch(RangeRequest {
            url: url(),
            range: Some(ByteRange::new(0, 10)),
        })
        .await
        .unwrap();
    origin
        .fetch(RangeRequest {
            url: url(),
            range: Some(ByteRange::new(10, 20)),
        })
        .await
        .unwrap();
    assert_eq!(
        origin.requested_ranges(),
        vec![ByteRange::new(0, 10), ByteRange::new(10, 20)]
    );
}

// Not in the Swift suite: the two switches `FakeOriginTests.swift` never
// exercises (`truncateAfterBytes`, `chunkSize`).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn origin_truncates_cleanly_at_configured_offset() {
    let behavior = FakeOriginBehavior {
        truncate_after_bytes: Some(40),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(1000), behavior);
    let response = origin.fetch(RangeRequest::new(url(), None)).await.unwrap();
    // The stream ends without an error, short of what was promised.
    assert_eq!(collect_body(response).await.unwrap(), payload(1000)[..40]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_size_controls_how_the_body_is_split() {
    use futures::StreamExt;

    let behavior = FakeOriginBehavior {
        chunk_size: 100,
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(payload(250), behavior);
    let response = origin.fetch(RangeRequest::new(url(), None)).await.unwrap();
    let sizes: Vec<usize> = response
        .body
        .map(|chunk| chunk.unwrap().len())
        .collect()
        .await;
    assert_eq!(sizes, vec![100, 100, 50]);
}
