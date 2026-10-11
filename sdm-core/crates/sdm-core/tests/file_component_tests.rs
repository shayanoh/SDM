use sdm_core::{Assembly, ByteRange, ComponentError, ComponentOrigin, FileComponent, SourceUrl};
use serde_json::json;

fn url(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

#[test]
fn component_reports_completion_against_its_own_size() {
    let mut c = FileComponent::new(url("https://x/v"), "v.f137.mp4").with_total_bytes(100);
    assert!(!c.is_complete());
    assert_eq!(c.fraction_completed(), 0.0);
    c.completed.insert(ByteRange::new(0, 100));
    assert!(c.is_complete());
    assert_eq!(c.fraction_completed(), 1.0);
}

#[test]
fn component_with_unknown_size_is_never_complete() {
    let mut c = FileComponent::new(url("https://x/v"), "v.mp4");
    c.completed.insert(ByteRange::new(0, 500));
    assert!(!c.is_complete());
    assert_eq!(c.fraction_completed(), 0.0);
}

#[test]
fn component_origin_round_trips_through_codable() {
    for origin in [
        ComponentOrigin::Http,
        ComponentOrigin::Resolved {
            format_id: "137".into(),
        },
        ComponentOrigin::Wholesale {
            format_selector: "bv*+ba/b".into(),
        },
    ] {
        let data = serde_json::to_vec(&origin).unwrap();
        assert_eq!(
            serde_json::from_slice::<ComponentOrigin>(&data).unwrap(),
            origin
        );
    }
}

#[test]
fn unknown_origin_kind_decodes_to_http() {
    let origin: ComponentOrigin = serde_json::from_value(json!({"kind": "martian"})).unwrap();
    assert_eq!(origin, ComponentOrigin::Http);
}

#[test]
fn legacy_resolved_origin_drops_extractor_and_video_id_keeps_format_id() {
    // Old synthesized shape from state.json written before the slim.
    let legacy = json!({"resolved": {"extractor": "youtube", "videoID": "abc", "formatID": "137"}});
    assert_eq!(
        serde_json::from_value::<ComponentOrigin>(legacy).unwrap(),
        ComponentOrigin::Resolved {
            format_id: "137".into()
        }
    );

    let legacy_http = json!({"http": {}});
    assert_eq!(
        serde_json::from_value::<ComponentOrigin>(legacy_http).unwrap(),
        ComponentOrigin::Http
    );
}

#[test]
fn file_component_round_trips_through_codable() {
    let mut c = FileComponent::new(url("https://x/v"), "v.f251.webm")
        .with_total_bytes(4096)
        .with_origin(ComponentOrigin::Resolved {
            format_id: "251".into(),
        });
    c.completed.insert(ByteRange::new(0, 1024));
    c.last_error = Some(ComponentError::new("connection reset"));
    let data = serde_json::to_vec(&c).unwrap();
    assert_eq!(serde_json::from_slice::<FileComponent>(&data).unwrap(), c);
}

#[test]
fn assembly_is_codable_as_a_string() {
    assert_eq!(
        serde_json::from_str::<Assembly>("\"mux\"").unwrap(),
        Assembly::Mux
    );
}
