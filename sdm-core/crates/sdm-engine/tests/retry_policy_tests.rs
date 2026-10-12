use std::time::Duration;

use sdm_engine::{DownloadError, FailureKind, RetryPolicy, TransportError};

#[test]
fn connection_drops_are_transient() {
    assert_eq!(
        RetryPolicy::new().classify(&TransportError::ConnectionDropped),
        FailureKind::Transient
    );
}

#[test]
fn server_errors_are_transient() {
    assert_eq!(
        RetryPolicy::new().classify(&TransportError::Http { status: 503 }),
        FailureKind::Transient
    );
    assert_eq!(
        RetryPolicy::new().classify(&DownloadError::ServerError { status: 500 }),
        FailureKind::Transient
    );
}

#[test]
fn not_found_is_permanent() {
    assert_eq!(
        RetryPolicy::new().classify(&DownloadError::ServerError { status: 404 }),
        FailureKind::Permanent {
            reason: "HTTP 404".to_string()
        }
    );
}

#[test]
fn forbidden_is_transient_because_signed_urls_expire() {
    assert_eq!(
        RetryPolicy::new().classify(&DownloadError::ServerError { status: 403 }),
        FailureKind::Transient
    );
}

#[test]
fn unknown_size_is_permanent() {
    assert_eq!(
        RetryPolicy::new().classify(&DownloadError::UnknownSize),
        FailureKind::Permanent {
            reason: "Server did not report a size".to_string()
        }
    );
}

#[test]
fn delay_grows_exponentially() {
    let policy = RetryPolicy::new()
        .with_max_attempts(5)
        .with_base_delay(Duration::from_millis(100));
    assert!(policy.delay_for_attempt(0) < policy.delay_for_attempt(1));
    assert!(policy.delay_for_attempt(1) < policy.delay_for_attempt(2));
}

#[test]
fn delay_is_deterministic_for_the_same_attempt() {
    let policy = RetryPolicy::new()
        .with_max_attempts(5)
        .with_base_delay(Duration::from_millis(100));
    assert_eq!(policy.delay_for_attempt(3), policy.delay_for_attempt(3));
}

#[test]
fn delay_is_capped_at_ceiling() {
    let policy = RetryPolicy::new()
        .with_max_attempts(20)
        .with_base_delay(Duration::from_secs(1))
        .with_max_delay(Duration::from_secs(30));
    assert!(policy.delay_for_attempt(19) <= Duration::from_secs(30));
}

#[test]
fn truncated_response_is_transient() {
    assert_eq!(
        RetryPolicy::new().classify(&DownloadError::TruncatedResponse {
            expected: 1000,
            received: 500
        }),
        FailureKind::Transient
    );
}

#[test]
fn url_expired_is_transient() {
    let kind = RetryPolicy::new().classify(&DownloadError::UrlExpired {
        format_id: "137".to_string(),
    });
    assert!(
        !matches!(kind, FailureKind::Permanent { .. }),
        "urlExpired must classify transient"
    );
}
