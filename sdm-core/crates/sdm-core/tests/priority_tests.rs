//! Beyond the Swift tests (which have none for `Priority.swift`): pins the
//! wire shapes Swift's synthesized `Codable` writes, using the Swift-written
//! fixture `fixtures/swift/state_rich.json`.

use sdm_core::{ItemState, Priority, RangeSet};
use serde_json::{Value, json};

fn rich_fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/swift/state_rich.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn items(fixture: &Value) -> Vec<&Value> {
    fixture["packages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["items"].as_array().unwrap())
        .collect()
}

#[test]
fn priority_orders_by_raw_value_and_lists_all_cases() {
    assert!(Priority::Lowest < Priority::Low);
    assert!(Priority::High < Priority::Highest);
    let raws: Vec<i64> = Priority::ALL_CASES.iter().map(|p| p.raw_value()).collect();
    assert_eq!(raws, [0, 1, 2, 3, 4]);
    assert!(Priority::ALL_CASES.is_sorted());
}

#[test]
fn priority_encodes_as_its_integer_raw_value() {
    for p in Priority::ALL_CASES {
        assert_eq!(serde_json::to_value(p).unwrap(), json!(p.raw_value()));
        assert_eq!(
            serde_json::from_value::<Priority>(json!(p.raw_value())).unwrap(),
            p
        );
    }
    assert!(serde_json::from_value::<Priority>(json!(5)).is_err());
    assert!(serde_json::from_value::<Priority>(json!(-1)).is_err());
    assert!(serde_json::from_value::<Priority>(json!("high")).is_err());
}

#[test]
fn item_state_wire_shapes_match_swift() {
    let cases = [
        (ItemState::Stopped, json!({"stopped": {}})),
        (ItemState::Queued, json!({"queued": {}})),
        (ItemState::Running, json!({"running": {}})),
        (ItemState::Completed, json!({"completed": {}})),
        (
            ItemState::Failed {
                reason: "HTTP 404".into(),
            },
            json!({"failed": {"reason": "HTTP 404"}}),
        ),
    ];
    for (state, wire) in cases {
        assert_eq!(serde_json::to_value(&state).unwrap(), wire);
        assert_eq!(serde_json::from_value::<ItemState>(wire).unwrap(), state);
    }
    assert!(serde_json::from_value::<ItemState>(json!({"paused": {}})).is_err());
    assert!(serde_json::from_value::<ItemState>(json!("stopped")).is_err());
    assert!(serde_json::from_value::<ItemState>(json!({"failed": {}})).is_err());
    assert!(serde_json::from_value::<ItemState>(json!({"stopped": {}, "queued": {}})).is_err());
}

#[test]
fn swift_written_fixture_state_priority_and_ranges_round_trip() {
    let fixture = rich_fixture();
    let mut seen_states = std::collections::HashSet::new();
    let mut seen_priorities = std::collections::HashSet::new();
    let mut seen_ranges = 0;
    for item in items(&fixture) {
        let state: ItemState = serde_json::from_value(item["state"].clone()).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), item["state"]);
        seen_states.insert(std::mem::discriminant(&state));
        if let Some(raw) = item.get("priority") {
            let priority: Priority = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(serde_json::to_value(priority).unwrap(), *raw);
            seen_priorities.insert(priority);
        }
        for component in item["components"].as_array().unwrap() {
            let set: RangeSet = serde_json::from_value(component["completed"].clone()).unwrap();
            assert_eq!(serde_json::to_value(&set).unwrap(), component["completed"]);
            seen_ranges += set.ranges().len();
        }
    }
    assert_eq!(
        seen_states.len(),
        5,
        "fixture covers all five ItemState cases"
    );
    assert_eq!(
        seen_priorities.len(),
        5,
        "fixture covers all five Priority cases"
    );
    assert!(seen_ranges > 0);
}
