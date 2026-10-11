use sdm_core::{
    Assembly, ByteRange, ComponentOrigin, DownloadItem, FileComponent, ItemState, RangeSet,
    SourceUrl,
};
use serde_json::json;

fn u(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

#[test]
fn convenience_init_builds_a_one_component_http_item() {
    let item = DownloadItem::http(u("https://x/f.bin"), "f.bin").with_total_bytes(1000);
    assert_eq!(item.components.len(), 1);
    assert_eq!(item.components[0].origin, ComponentOrigin::Http);
    assert_eq!(item.components[0].part_filename, "f.bin");
    assert_eq!(item.output_filename, "f.bin");
    assert_eq!(item.assembly, Assembly::None);
    assert_eq!(item.url(), &u("https://x/f.bin"));
    assert_eq!(item.filename(), "f.bin");
    assert_eq!(item.total_bytes(), Some(1000));
}

#[test]
fn one_component_accessors_are_byte_for_byte_unchanged() {
    let mut item = DownloadItem::http(u("https://x/f.bin"), "f.bin").with_total_bytes(1000);
    item.set_completed(RangeSet::from_ranges(&[ByteRange::new(0, 400)]));
    assert_eq!(item.completed().total_bytes(), 400);
    assert_eq!(item.fraction_completed(), 0.4);
    assert!(!item.is_complete());
    item.set_is_resumable(Some(true));
    assert_eq!(item.is_resumable(), Some(true));
    assert_eq!(item.components[0].is_resumable, Some(true));
    item.set_total_bytes(Some(400));
    assert!(item.is_complete());
}

#[test]
fn concatenated_completed_shifts_later_components_by_prior_sizes() {
    let video = FileComponent::new(u("https://gv/v"), "t.f137.mp4")
        .with_total_bytes(100)
        .with_completed(RangeSet::from_ranges(&[ByteRange::new(0, 60)]))
        .with_origin(ComponentOrigin::Http);
    let audio = FileComponent::new(u("https://gv/a"), "t.f251.webm")
        .with_total_bytes(20)
        .with_completed(RangeSet::from_ranges(&[ByteRange::new(0, 10)]))
        .with_origin(ComponentOrigin::Http);
    let item = DownloadItem::new(vec![video, audio], "t.mp4").with_assembly(Assembly::Mux);
    assert_eq!(item.total_bytes(), Some(120));
    assert_eq!(
        item.completed().ranges(),
        [ByteRange::new(0, 60), ByteRange::new(100, 110)]
    );
    assert_eq!(item.fraction_completed(), 70.0 / 120.0);
    assert!(!item.is_complete());
}

#[test]
fn is_resumable_is_false_if_any_component_is_false() {
    let mut a = FileComponent::new(u("https://x/a"), "a").with_is_resumable(Some(true));
    let b = FileComponent::new(u("https://x/b"), "b").with_is_resumable(Some(false));
    let mut item =
        DownloadItem::new(vec![a.clone(), b.clone()], "out.mp4").with_assembly(Assembly::Mux);
    assert_eq!(item.is_resumable(), Some(false));
    a.is_resumable = None;
    item = DownloadItem::new(vec![a, b], "out.mp4").with_assembly(Assembly::Mux);
    assert_eq!(item.is_resumable(), Some(false));
    let c = FileComponent::new(u("https://x/c"), "c").with_is_resumable(None);
    let d = FileComponent::new(u("https://x/d"), "d").with_is_resumable(Some(true));
    item = DownloadItem::new(vec![c, d], "out.mp4").with_assembly(Assembly::Mux);
    assert_eq!(item.is_resumable(), None);
}

#[test]
fn new_shape_codable_round_trips() {
    let item = DownloadItem::new(
        vec![
            FileComponent::new(u("https://gv/v"), "t.f137.mp4")
                .with_total_bytes(100)
                .with_origin(ComponentOrigin::Resolved {
                    format_id: "137".into(),
                }),
            FileComponent::new(u("https://gv/a"), "t.f251.webm")
                .with_total_bytes(20)
                .with_origin(ComponentOrigin::Resolved {
                    format_id: "251".into(),
                }),
        ],
        "t.mp4",
    )
    .with_assembly(Assembly::Mux)
    .with_state(ItemState::Stopped)
    .with_metadata("Streaming · 1080p · h264 · mp4 · twitch");
    let data = serde_json::to_vec(&item).unwrap();
    let decoded: DownloadItem = serde_json::from_slice(&data).unwrap();
    assert_eq!(decoded, item);
    assert_eq!(
        decoded.metadata.as_deref(),
        Some("Streaming · 1080p · h264 · mp4 · twitch")
    );
}

#[test]
fn metadata_defaults_to_nil_and_survives_absence() {
    let item = DownloadItem::http(u("https://x/f.bin"), "f.bin");
    assert_eq!(item.metadata, None);
    let data = serde_json::to_vec(&item).unwrap();
    assert_eq!(
        serde_json::from_slice::<DownloadItem>(&data)
            .unwrap()
            .metadata,
        None
    );
}

#[test]
fn legacy_flat_codable_decodes_to_one_component_item() {
    let legacy = json!({
        "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
        "url": "https://x/f.bin",
        "filename": "f.bin",
        "totalBytes": 2048,
        "completed": { "ranges": [ { "start": 0, "end": 1024 } ] },
        "state": { "stopped": {} },
        "isEnabled": true,
        "isResumable": true,
        "position": 3
    });
    let item: DownloadItem = serde_json::from_value(legacy).unwrap();
    assert_eq!(item.components.len(), 1);
    assert_eq!(item.components[0].origin, ComponentOrigin::Http);
    assert_eq!(item.url(), &u("https://x/f.bin"));
    assert_eq!(item.output_filename, "f.bin");
    assert_eq!(item.total_bytes(), Some(2048));
    assert_eq!(item.completed().total_bytes(), 1024);
    assert_eq!(item.is_resumable(), Some(true));
    assert_eq!(item.state, ItemState::Stopped);
    assert_eq!(item.position, 3);
}

#[test]
fn source_url_defaults_to_component_zero_but_can_be_overridden() {
    let http = DownloadItem::http(u("https://x/f.bin"), "f.bin");
    assert_eq!(http.url(), &u("https://x/f.bin"));
    assert_eq!(http.source_url, http.components[0].url);

    let media = DownloadItem::new(
        vec![
            FileComponent::new(u("https://gv/v"), "t.f137.mp4"),
            FileComponent::new(u("https://gv/a"), "t.f251.webm"),
        ],
        "t.mp4",
    )
    .with_source_url(u("https://youtu.be/abc"))
    .with_assembly(Assembly::Mux);
    assert_eq!(media.url(), &u("https://youtu.be/abc"));
    assert_eq!(media.components[0].url, u("https://gv/v"));

    let data = serde_json::to_vec(&media).unwrap();
    assert_eq!(
        serde_json::from_slice::<DownloadItem>(&data).unwrap().url(),
        &u("https://youtu.be/abc")
    );
}

#[test]
fn legacy_item_without_source_url_falls_back_to_the_flat_url() {
    let legacy = json!({
        "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427", "url": "https://x/f.bin",
        "filename": "f.bin", "state": {"stopped": {}}, "isEnabled": true, "position": 0
    });
    let item: DownloadItem = serde_json::from_value(legacy).unwrap();
    assert_eq!(item.url(), &u("https://x/f.bin"));
    assert_eq!(item.source_url, u("https://x/f.bin"));
}
