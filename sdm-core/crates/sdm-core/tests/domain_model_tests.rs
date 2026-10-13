use sdm_core::{ByteRange, DownloadItem, DownloadPackage, ItemState, Priority, SourceUrl};
use serde_json::json;

fn u(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

fn item() -> DownloadItem {
    DownloadItem::http(u("https://example.com/a.bin"), "a.bin")
}

#[test]
fn priorities_compare_by_rank() {
    assert!(Priority::Highest > Priority::Normal);
    assert!(Priority::Lowest < Priority::Low);
    assert_eq!(Priority::ALL_CASES.len(), 5);
}

#[test]
fn item_defaults_to_queued_and_enabled() {
    let item = item();
    assert_eq!(item.state, ItemState::Queued);
    assert!(item.is_enabled);
    assert_eq!(item.priority, None);
    assert!(item.completed().ranges().is_empty());
    assert_eq!(item.total_bytes(), None);
    assert_eq!(item.is_resumable(), None);
}

#[test]
fn item_progress_is_zero_when_size_unknown() {
    assert_eq!(item().fraction_completed(), 0.0);
}

#[test]
fn item_progress_reflects_completed_ranges() {
    let mut item = item();
    item.set_total_bytes(Some(1000));
    let mut completed = item.completed();
    completed.insert(ByteRange::new(0, 250));
    item.set_completed(completed);
    assert_eq!(item.fraction_completed(), 0.25);
}

#[test]
fn effective_priority_falls_back_to_package() {
    let mut package = DownloadPackage::new("Season 1");
    package.priority = Priority::High;
    assert_eq!(package.effective_priority(&item()), Priority::High);
}

#[test]
fn item_priority_overrides_package() {
    let mut package = DownloadPackage::new("Season 1");
    package.priority = Priority::Low;
    let mut item = item();
    item.priority = Some(Priority::Highest);
    assert_eq!(package.effective_priority(&item), Priority::Highest);
}

#[test]
fn failed_state_carries_reason() {
    let state = ItemState::Failed {
        reason: "404 Not Found".into(),
    };
    let data = serde_json::to_vec(&state).unwrap();
    assert_eq!(serde_json::from_slice::<ItemState>(&data).unwrap(), state);
}

#[test]
fn package_round_trips_through_codable() {
    let mut package = DownloadPackage::new("Season 1");
    package.items = vec![item()];
    let data = serde_json::to_vec(&package).unwrap();
    assert_eq!(
        serde_json::from_slice::<DownloadPackage>(&data).unwrap(),
        package
    );
}

#[test]
fn download_package_note_defaults_to_nil_and_round_trips() {
    let plain = DownloadPackage::new("P");
    assert_eq!(plain.note, None);

    let annotated = DownloadPackage::new("Playlist").with_note("50 of 320 videos");
    assert_eq!(annotated.note.as_deref(), Some("50 of 320 videos"));
    let data = serde_json::to_vec(&annotated).unwrap();
    assert_eq!(
        serde_json::from_slice::<DownloadPackage>(&data).unwrap(),
        annotated
    );

    // Old snapshot without the key still decodes.
    let legacy = json!({
        "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427", "name": "P", "items": [],
        "priority": 2, "position": 0
    });
    let decoded: DownloadPackage = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.note, None);
}
