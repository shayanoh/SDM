mod support;

use sdm_grabber::{FileSignature, ProbedLink, Stage, Verdict, VerdictRules};
use support::su;

struct Spec {
    status_code: Option<i64>,
    content_length: Option<i64>,
    content_type: Option<&'static str>,
    filename: &'static str,
    signature: Option<FileSignature>,
    transport_failed: bool,
    original_host: &'static str,
    final_host: &'static str,
    final_path: &'static str,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            status_code: Some(200),
            content_length: Some(5_000_000),
            content_type: Some("video/mp4"),
            filename: "movie.mp4",
            signature: None,
            transport_failed: false,
            original_host: "example.com",
            final_host: "example.com",
            final_path: "/movie.mp4",
        }
    }
}

fn link(spec: Spec) -> ProbedLink {
    let mut probed = ProbedLink::new(su(&format!("https://{}/movie.mp4", spec.original_host)))
        .with_final_url(su(&format!(
            "https://{}{}",
            spec.final_host, spec.final_path
        )))
        .with_stage(Stage::Done);
    probed.status_code = spec.status_code;
    probed.content_length = spec.content_length;
    probed.content_type = spec.content_type.map(str::to_owned);
    probed.suggested_filename = Some(spec.filename.to_owned());
    probed.sniffed_signature = spec.signature;
    probed.transport_failed = spec.transport_failed;
    probed
}

#[test]
fn online_for_a_plausible_successful_response() {
    assert_eq!(
        VerdictRules::evaluate(&link(Spec::default())),
        Verdict::Online
    );
}

#[test]
fn offline_for_non_2xx_status() {
    let spec = Spec {
        status_code: Some(404),
        ..Spec::default()
    };
    assert_eq!(VerdictRules::evaluate(&link(spec)), Verdict::Offline);
}

#[test]
fn faulty_for_html_body_on_a_media_extension() {
    let spec = Spec {
        content_type: Some("text/html"),
        ..Spec::default()
    };
    assert_eq!(
        VerdictRules::evaluate(&link(spec)),
        Verdict::Faulty {
            reason: "html, not mp4".into()
        }
    );
}

#[test]
fn faulty_for_implausibly_small_size() {
    let spec = Spec {
        content_length: Some(100),
        filename: "archive.zip",
        ..Spec::default()
    };
    assert_eq!(
        VerdictRules::evaluate(&link(spec)),
        Verdict::Faulty {
            reason: "too small to be a real zip".into()
        }
    );
}

#[test]
fn faulty_for_signature_contradicting_extension() {
    let spec = Spec {
        signature: Some(FileSignature::Html),
        ..Spec::default()
    };
    assert_eq!(
        VerdictRules::evaluate(&link(spec)),
        Verdict::Faulty {
            reason: "file signature does not match .mp4".into()
        }
    );
}

#[test]
fn check_failed_when_the_transport_never_connected() {
    let spec = Spec {
        transport_failed: true,
        ..Spec::default()
    };
    assert_eq!(VerdictRules::evaluate(&link(spec)), Verdict::CheckFailed);
}

#[test]
fn check_failed_when_status_was_never_captured() {
    let spec = Spec {
        status_code: None,
        ..Spec::default()
    };
    assert_eq!(VerdictRules::evaluate(&link(spec)), Verdict::CheckFailed);
}

#[test]
fn faulty_for_redirect_to_a_suspicious_path_on_a_different_host() {
    let spec = Spec {
        content_type: Some("application/octet-stream"),
        final_host: "sketchy-cdn.example",
        final_path: "/link-expired",
        ..Spec::default()
    };
    assert_eq!(
        VerdictRules::evaluate(&link(spec)),
        Verdict::Faulty {
            reason: "redirected to sketchy-cdn.example".into()
        }
    );
}

#[test]
fn online_for_a_small_non_media_file() {
    let spec = Spec {
        content_length: Some(50),
        content_type: Some("text/plain"),
        filename: "notes.txt",
        ..Spec::default()
    };
    assert_eq!(VerdictRules::evaluate(&link(spec)), Verdict::Online);
}
