use std::path::Path;

use sdm_core::{ByteRange, RangeSet, SourceUrl};
use sdm_engine::ResumeSidecar;
use serde_json::Value;

fn url(string: &str) -> SourceUrl {
    SourceUrl::new(string).unwrap()
}

fn sample_sidecar() -> ResumeSidecar {
    ResumeSidecar::new(
        url("https://example.com/a.bin"),
        None,
        1000,
        Some("etag-1".to_string()),
        RangeSet::from_ranges(&[ByteRange::new(0, 250)]),
    )
}

fn fixture(name: &str) -> String {
    let path = format!("{}/../../fixtures/swift/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn sidecar_url_uses_sdmpart_extension() {
    let path = ResumeSidecar::path_for(Path::new("/tmp/movie.mp4"));
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        "movie.mp4.sdmpart"
    );
}

#[test]
fn sidecar_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin.sdmpart");

    sample_sidecar().save(&path).unwrap();
    assert_eq!(ResumeSidecar::load(&path), Some(sample_sidecar()));
}

#[test]
fn loading_missing_sidecar_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(ResumeSidecar::load(&dir.path().join("nope.sdmpart")), None);
}

#[test]
fn loading_corrupt_sidecar_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin.sdmpart");
    std::fs::write(&path, "not json at all").unwrap();
    assert_eq!(ResumeSidecar::load(&path), None);
}

#[test]
fn loading_future_format_version_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin.sdmpart");
    sample_sidecar()
        .with_format_version(999)
        .save(&path)
        .unwrap();
    assert_eq!(ResumeSidecar::load(&path), None);
}

#[test]
fn saving_overwrites_previous_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin.sdmpart");

    sample_sidecar().save(&path).unwrap();
    let mut updated = sample_sidecar();
    updated.completed.insert(ByteRange::new(500, 900));
    updated.save(&path).unwrap();

    assert_eq!(
        ResumeSidecar::load(&path).unwrap().completed.total_bytes(),
        650
    );
}

#[test]
fn matches_rejects_changed_validator() {
    let sidecar = sample_sidecar();
    assert!(sidecar.matches(1000, Some("etag-1")));
    assert!(!sidecar.matches(1000, Some("etag-2")));
    assert!(!sidecar.matches(2000, Some("etag-1")));
}

#[test]
fn matches_accepts_absent_validator_on_both_sides() {
    let sidecar = ResumeSidecar::new(
        url("https://example.com/a.bin"),
        None,
        1000,
        None,
        RangeSet::new(),
    );
    assert!(sidecar.matches(1000, None));
}

#[test]
fn loads_sidecar_written_by_swift() {
    for name in ["sidecar_rich.json", "sidecar_minimal.json"] {
        let text = fixture(name);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sdmpart");
        std::fs::write(&path, &text).unwrap();

        let loaded = ResumeSidecar::load(&path).unwrap_or_else(|| panic!("{name} did not load"));
        let written = serde_json::to_value(&loaded).unwrap();
        let expected: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(written, expected, "{name}");

        // And what Rust saves loads back.
        let resaved = dir.path().join("y.sdmpart");
        loaded.save(&resaved).unwrap();
        assert_eq!(ResumeSidecar::load(&resaved), Some(loaded));
    }
}

#[test]
fn resumable_ranges_trusts_completed_only_in_the_same_boot_session() {
    let completed = RangeSet::from_ranges(&[ByteRange::new(0, 100)]);
    let durable = RangeSet::from_ranges(&[ByteRange::new(0, 40)]);

    let legacy = ResumeSidecar::new(url("https://x/a"), None, 100, None, completed.clone());
    assert_eq!(legacy.resumable_ranges(Some("b")), completed);

    let sidecar = legacy
        .clone()
        .with_durable_completed(durable.clone())
        .with_boot_session_id("a");
    assert_eq!(sidecar.resumable_ranges(Some("a")), completed);
    assert_eq!(sidecar.resumable_ranges(Some("b")), durable);
    assert_eq!(sidecar.resumable_ranges(None), durable);
}

#[cfg(target_os = "macos")]
#[test]
fn current_boot_session_id_is_readable_and_stable() {
    let id = ResumeSidecar::current_boot_session_id().expect("kern.bootsessionuuid");
    assert!(!id.is_empty());
    assert_eq!(ResumeSidecar::current_boot_session_id(), Some(id));
}
