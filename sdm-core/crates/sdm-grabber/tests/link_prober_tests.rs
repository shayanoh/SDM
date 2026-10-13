mod support;

use sdm_grabber::{
    FakeProbeOrigin, FakeProbeOriginBehavior, LinkProber, ProbeError, ProbeMethod, Stage,
};
use std::sync::Arc;
use support::{su, within};

const URL: &str = "https://example.com/movie.mp4";

fn headers(fields: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn origin_with(behavior: FakeProbeOriginBehavior) -> Arc<FakeProbeOrigin> {
    let origin = Arc::new(FakeProbeOrigin::new());
    origin.set_behavior(behavior, &su(URL));
    origin
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_captures_head_response_fields() {
    within(async {
        let behavior = FakeProbeOriginBehavior {
            status_code: 200,
            headers: headers(&[
                ("content-length", "5000000"),
                ("content-type", "video/mp4; charset=binary"),
                ("etag", "\"abc123\""),
                (
                    "content-disposition",
                    "attachment; filename=\"Real Movie.mp4\"",
                ),
            ]),
            ..FakeProbeOriginBehavior::default()
        };
        let origin = origin_with(behavior);

        let link = LinkProber::new(origin, false).probe(&su(URL)).await;
        assert_eq!(link.status_code, Some(200));
        assert_eq!(link.content_length, Some(5_000_000));
        assert_eq!(link.content_type.as_deref(), Some("video/mp4"));
        assert_eq!(link.validator.as_deref(), Some("\"abc123\""));
        assert_eq!(link.suggested_filename.as_deref(), Some("Real Movie.mp4"));
        assert!(!link.accepts_ranges);
        assert_eq!(link.stage, Stage::Done);
        assert!(!link.transport_failed);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_falls_back_to_ranged_get_when_head_is_rejected() {
    within(async {
        let behavior = FakeProbeOriginBehavior {
            rejects_head: true,
            status_code: 206,
            ..FakeProbeOriginBehavior::default()
        };
        let origin = origin_with(behavior);

        let link = LinkProber::new(origin.clone(), false).probe(&su(URL)).await;
        assert!(link.accepts_ranges);
        let methods: Vec<ProbeMethod> = origin.request_log().iter().map(|r| r.method).collect();
        assert_eq!(methods, vec![ProbeMethod::Head, ProbeMethod::Get]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_captures_total_from_content_range_on_fallback_get() {
    within(async {
        let behavior = FakeProbeOriginBehavior {
            rejects_head: true,
            status_code: 206,
            headers: headers(&[
                ("content-range", "bytes 0-0/123456"),
                ("content-length", "1"),
            ]),
            ..FakeProbeOriginBehavior::default()
        };
        let origin = origin_with(behavior);

        let link = LinkProber::new(origin, false).probe(&su(URL)).await;
        assert_eq!(link.content_length, Some(123456));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_preserves_status_code_verbatim() {
    within(async {
        let behavior = FakeProbeOriginBehavior {
            status_code: 404,
            ..FakeProbeOriginBehavior::default()
        };
        let origin = origin_with(behavior);

        let link = LinkProber::new(origin, false).probe(&su(URL)).await;
        assert_eq!(link.status_code, Some(404));
        assert_eq!(link.stage, Stage::Done);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_marks_transport_failure_when_both_attempts_throw() {
    within(async {
        let behavior = FakeProbeOriginBehavior {
            error: Some(ProbeError::TimedOut),
            ..FakeProbeOriginBehavior::default()
        };
        let origin = origin_with(behavior);

        let link = LinkProber::new(origin, false).probe(&su(URL)).await;
        assert!(link.transport_failed);
        assert_eq!(link.status_code, None);
        assert_eq!(link.stage, Stage::Done);
    })
    .await;
}
