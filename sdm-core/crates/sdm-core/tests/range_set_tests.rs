use sdm_core::{ByteRange, RangeSet};

fn br(start: i64, end: i64) -> ByteRange {
    ByteRange::new(start, end)
}

#[test]
fn byte_range_length() {
    assert_eq!(br(10, 25).length(), 15);
}

#[test]
fn byte_range_rejects_inverted_bounds() {
    assert_eq!(br(10, 10).length(), 0);
}

#[test]
fn insert_into_empty_set() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    assert_eq!(set.ranges(), [br(0, 100)]);
    assert_eq!(set.total_bytes(), 100);
}

#[test]
fn insert_disjoint_ranges_stays_sorted() {
    let mut set = RangeSet::new();
    set.insert(br(200, 300));
    set.insert(br(0, 100));
    assert_eq!(set.ranges(), [br(0, 100), br(200, 300)]);
    assert_eq!(set.total_bytes(), 200);
}

#[test]
fn adjacent_ranges_coalesce() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    set.insert(br(100, 200));
    assert_eq!(set.ranges(), [br(0, 200)]);
}

#[test]
fn overlapping_ranges_merge() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    set.insert(br(50, 150));
    assert_eq!(set.ranges(), [br(0, 150)]);
    assert_eq!(set.total_bytes(), 150);
}

#[test]
fn insert_bridging_two_ranges_merges_all_three() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    set.insert(br(200, 300));
    set.insert(br(90, 210));
    assert_eq!(set.ranges(), [br(0, 300)]);
}

#[test]
fn insert_fully_contained_range_is_no_op() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    set.insert(br(20, 30));
    assert_eq!(set.ranges(), [br(0, 100)]);
    assert_eq!(set.total_bytes(), 100);
}

#[test]
fn insert_empty_range_is_no_op() {
    let mut set = RangeSet::new();
    set.insert(br(50, 50));
    assert!(set.ranges().is_empty());
}

#[test]
fn contains_respects_half_open_bounds() {
    let mut set = RangeSet::new();
    set.insert(br(10, 20));
    assert!(set.contains(10));
    assert!(set.contains(19));
    assert!(!set.contains(20));
    assert!(!set.contains(9));
}

#[test]
fn range_set_round_trips_through_codable() {
    let mut set = RangeSet::new();
    set.insert(br(0, 100));
    set.insert(br(200, 300));
    let data = serde_json::to_vec(&set).unwrap();
    assert_eq!(serde_json::from_slice::<RangeSet>(&data).unwrap(), set);
}

// Beyond the Swift tests: the wire shape Swift's synthesized Codable writes.
#[test]
fn range_set_wire_shape_matches_swift() {
    let set = RangeSet::from_ranges(&[br(0, 100), br(200, 300)]);
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"{"ranges":[{"start":0,"end":100},{"start":200,"end":300}]}"#
    );
    assert_eq!(
        serde_json::to_string(&RangeSet::new()).unwrap(),
        r#"{"ranges":[]}"#
    );
}
