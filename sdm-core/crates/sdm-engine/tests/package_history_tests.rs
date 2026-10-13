//! Port of Swift's `PackageHistoryTests.swift` (2 tests).

use sdm_core::{ItemState, Priority, RangeSet, SourceUrl};
use sdm_engine::{ItemSnapshot, PackageSnapshot};
use uuid::Uuid;

fn make_item_snapshot(speed_history: Vec<f64>) -> ItemSnapshot {
    let bytes_per_second = speed_history.last().copied().unwrap_or(0.0);
    ItemSnapshot::new(
        Uuid::new_v4(),
        SourceUrl::new("https://example.com/a.bin").unwrap(),
        "a.bin",
        None,
        RangeSet::new(),
        ItemState::Running,
        true,
        None,
        1,
        1,
        bytes_per_second,
        speed_history,
    )
}

/// Items can join mid-run with shorter histories (a later addition, or a
/// sampler that has ticked fewer times); the sum aligns every array's most
/// recent sample to the same trailing edge, front-padding the shorter ones
/// with zero rather than misaligning by index.
#[test]
fn package_history_sums_member_item_histories_aligned_to_their_trailing_edge() {
    let a = make_item_snapshot(vec![10.0, 20.0, 30.0]);
    let b = make_item_snapshot(vec![1.0, 2.0]);
    let package = PackageSnapshot::new(Uuid::new_v4(), "P", Priority::Normal, vec![a, b], 0.0, 0);
    assert_eq!(package.bytes_per_second_history(), [10.0, 21.0, 32.0]);
}

#[test]
fn package_history_of_no_items_is_empty() {
    let package = PackageSnapshot::new(Uuid::new_v4(), "P", Priority::Normal, vec![], 0.0, 0);
    assert!(package.bytes_per_second_history().is_empty());
}
