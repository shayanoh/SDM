//! Which transitions deserve a notification. A port of the SwiftUI app's
//! `EngineController.notifyChanges`.

use std::collections::HashMap;

use sdm_core::ItemState;
use sdm_engine::EngineSnapshot;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    Finished { filename: String },
    Failed { filename: String, reason: String },
}

/// Notifications due for the step from `old` to `new`.
///
/// Only packages whose `revision` changed can hold a transition. Both sides are
/// collected (an item moved between packages changed its old package and its
/// new one), so the item's previous state is always found and a move never
/// reads as "finished".
pub(crate) fn notifications_between(
    old: &EngineSnapshot,
    new: &EngineSnapshot,
) -> Vec<Notification> {
    if old.generation == new.generation {
        return Vec::new();
    }
    let old_by_id: HashMap<Uuid, u64> = old.packages.iter().map(|p| (p.id, p.revision)).collect();
    let new_by_id: HashMap<Uuid, u64> = new.packages.iter().map(|p| (p.id, p.revision)).collect();

    let changed_new: Vec<_> = new
        .packages
        .iter()
        .filter(|p| old_by_id.get(&p.id) != Some(&p.revision))
        .collect();
    if changed_new.is_empty() {
        return Vec::new();
    }
    let old_states: HashMap<Uuid, &ItemState> = old
        .packages
        .iter()
        .filter(|p| new_by_id.get(&p.id) != Some(&p.revision))
        .flat_map(|p| p.items.iter().map(|i| (i.id, &i.state)))
        .collect();

    let mut out = Vec::new();
    for item in changed_new.iter().flat_map(|p| p.items.iter()) {
        if old_states.get(&item.id).copied() == Some(&item.state) {
            continue;
        }
        match &item.state {
            ItemState::Completed => out.push(Notification::Finished {
                filename: item.filename.clone(),
            }),
            ItemState::Failed { reason } => out.push(Notification::Failed {
                filename: item.filename.clone(),
                reason: reason.clone(),
            }),
            _ => {}
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    fn item(id: Uuid, name: &str, state: ItemState) -> ItemSnapshot {
        ItemSnapshot::new(
            id,
            SourceUrl::new("https://e.com/x").unwrap(),
            name,
            Some(10),
            RangeSet::new(),
            state,
            true,
            Some(true),
            0,
            1,
            0.0,
            vec![],
        )
    }

    fn package(id: Uuid, revision: u64, items: Vec<ItemSnapshot>) -> PackageSnapshot {
        PackageSnapshot::new(id, "P", Priority::Normal, items, 0.0, revision)
    }

    fn snapshot(generation: u64, packages: Vec<PackageSnapshot>) -> EngineSnapshot {
        EngineSnapshot::new(packages, 0.0, vec![], generation)
    }

    #[test]
    fn a_transition_to_completed_notifies_once() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![package(p, 1, vec![item(i, "a.bin", ItemState::Running)])],
        );
        let new = snapshot(
            2,
            vec![package(p, 2, vec![item(i, "a.bin", ItemState::Completed)])],
        );
        assert_eq!(
            notifications_between(&old, &new),
            vec![Notification::Finished {
                filename: "a.bin".into()
            }]
        );
        // The same transition seen again (old == new) is not a transition.
        assert!(notifications_between(&new, &new).is_empty());
    }

    #[test]
    fn a_failure_carries_its_reason() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![package(p, 1, vec![item(i, "a.bin", ItemState::Running)])],
        );
        let new = snapshot(
            2,
            vec![package(
                p,
                2,
                vec![item(
                    i,
                    "a.bin",
                    ItemState::Failed {
                        reason: "404".into(),
                    },
                )],
            )],
        );
        assert_eq!(
            notifications_between(&old, &new),
            vec![Notification::Failed {
                filename: "a.bin".into(),
                reason: "404".into()
            }]
        );
    }

    #[test]
    fn an_unchanged_package_is_ignored_even_if_it_holds_a_completed_item() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![package(p, 5, vec![item(i, "a.bin", ItemState::Completed)])],
        );
        let new = snapshot(
            2,
            vec![package(p, 5, vec![item(i, "a.bin", ItemState::Completed)])],
        );
        assert!(notifications_between(&old, &new).is_empty());
    }

    #[test]
    fn moving_a_finished_item_between_packages_does_not_read_as_finished() {
        let (p1, p2, i) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![
                package(p1, 1, vec![item(i, "a.bin", ItemState::Completed)]),
                package(p2, 1, vec![]),
            ],
        );
        let new = snapshot(
            2,
            vec![
                package(p1, 2, vec![]),
                package(p2, 2, vec![item(i, "a.bin", ItemState::Completed)]),
            ],
        );
        assert!(notifications_between(&old, &new).is_empty());
    }

    #[test]
    fn a_newly_added_item_already_in_a_terminal_state_notifies() {
        // Mirrors the SwiftUI app: an id missing from the old snapshot compares
        // unequal to any state.
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![package(p, 1, vec![])]);
        let new = snapshot(
            2,
            vec![package(p, 2, vec![item(i, "a.bin", ItemState::Completed)])],
        );
        assert_eq!(notifications_between(&old, &new).len(), 1);
    }
}
