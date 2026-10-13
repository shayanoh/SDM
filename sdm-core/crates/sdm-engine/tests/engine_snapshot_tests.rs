//! Rust-only: pins the equality contract of the snapshot fields Swift does
//! not have (`aggregate`, `revision`, `figures`, `generation`).

use sdm_core::{ItemState, Priority, RangeSet, SourceUrl};
use sdm_engine::{EngineSnapshot, ItemSnapshot, PackageSnapshot};
use uuid::Uuid;

fn item(id: Uuid) -> ItemSnapshot {
    ItemSnapshot::new(
        id,
        SourceUrl::new("https://example.com/file").unwrap(),
        "file",
        Some(1000),
        RangeSet::new(),
        ItemState::Queued,
        true,
        Some(true),
        0,
        4,
        0.0,
        vec![],
    )
}

#[test]
fn part_filenames_default_to_the_filename() {
    let snapshot = item(Uuid::new_v4());
    assert_eq!(snapshot.part_filenames, ["file"]);
    assert_eq!(
        snapshot.clone().with_part_filenames(vec![]).part_filenames,
        ["file"]
    );
}

#[test]
fn package_equality_ignores_revision_and_aggregate_but_content_eq_does_not() {
    let id = Uuid::new_v4();
    let item = item(Uuid::new_v4());
    let a = PackageSnapshot::new(id, "p", Priority::Normal, vec![item.clone()], 0.0, 1);
    let b = PackageSnapshot::new(id, "p", Priority::Normal, vec![item], 500.0, 2);
    assert_eq!(a, b);
    assert!(!a.content_eq(&b));
    assert!(a.content_eq(&a.clone()));
}

#[test]
fn engine_equality_ignores_generation_but_content_eq_does_not() {
    let a = EngineSnapshot::new(vec![], 0.0, vec![], 1);
    let b = EngineSnapshot::new(vec![], 0.0, vec![], 2);
    assert_eq!(a, b);
    assert!(!a.content_eq(&b));
    assert!(a.content_eq(&a.clone()));
}
