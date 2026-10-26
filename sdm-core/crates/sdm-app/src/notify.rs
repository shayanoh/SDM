//! Which transitions deserve a notification. A port of the SwiftUI app's
//! `EngineController.notifyChanges`.

use std::collections::{HashMap, HashSet};

use sdm_core::ItemState;
use sdm_engine::{EngineSnapshot, PackageSnapshot};
use uuid::Uuid;

use crate::protocol::SettingsDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    Finished { filename: String },
    PackageFinished { name: String },
    Failed { filename: String, reason: String },
    LinksGrabbed { count: u32 },
}

/// The view a notification opens when it is clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Downloads,
    Completed,
    Linkgrabber,
}

impl Route {
    /// The web `Tab` value for this view.
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Downloads => "downloads",
            Route::Completed => "completed",
            Route::Linkgrabber => "linkgrabber",
        }
    }
}

impl Notification {
    pub fn title_body(&self) -> (String, String) {
        match self {
            Notification::Finished { filename } => {
                ("Download finished".to_owned(), filename.clone())
            }
            Notification::PackageFinished { name } => ("Package finished".to_owned(), name.clone()),
            Notification::Failed { filename, reason } => (
                "Download failed".to_owned(),
                format!("{filename}: {reason}"),
            ),
            Notification::LinksGrabbed { count } => (
                format!("{count} links grabbed"),
                "Waiting for confirmation in the Linkgrabber".to_owned(),
            ),
        }
    }

    pub fn route(&self) -> Route {
        match self {
            Notification::Finished { .. } | Notification::PackageFinished { .. } => {
                Route::Completed
            }
            Notification::Failed { .. } => Route::Downloads,
            Notification::LinksGrabbed { .. } => Route::Linkgrabber,
        }
    }

    /// Whether the setting for this type lets it through.
    pub fn enabled(&self, settings: &SettingsDto) -> bool {
        match self {
            Notification::Finished { .. } => settings.notify_download_finished,
            Notification::PackageFinished { .. } => settings.notify_package_finished,
            Notification::Failed { .. } => settings.notify_download_failed,
            Notification::LinksGrabbed { count } => settings.notify_links_grabbed && *count > 0,
        }
    }
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

    let old_packages: HashMap<Uuid, &PackageSnapshot> =
        old.packages.iter().map(|p| (p.id, p)).collect();
    let mut out = Vec::new();
    // A package is announced only when one of its own items completed in this
    // step, so moving finished items around never reads as a finished package.
    let mut with_a_new_completion: HashSet<Uuid> = HashSet::new();
    for package in &changed_new {
        for item in &package.items {
            if old_states.get(&item.id).copied() == Some(&item.state) {
                continue;
            }
            match &item.state {
                ItemState::Completed => {
                    with_a_new_completion.insert(package.id);
                    out.push(Notification::Finished {
                        filename: item.filename.clone(),
                    });
                }
                ItemState::Failed { reason } => out.push(Notification::Failed {
                    filename: item.filename.clone(),
                    reason: reason.clone(),
                }),
                _ => {}
            }
        }
    }
    for package in &changed_new {
        let was_done = old_packages
            .get(&package.id)
            .is_some_and(|old| is_finished(old));
        // A package of one item is its item: announcing both says it twice.
        if package.items.len() > 1
            && with_a_new_completion.contains(&package.id)
            && is_finished(package)
            && !was_done
        {
            out.push(Notification::PackageFinished {
                name: package.name.clone(),
            });
        }
    }
    out
}

/// A package is finished when it has items and every one is completed.
fn is_finished(package: &PackageSnapshot) -> bool {
    !package.items.is_empty()
        && package
            .items
            .iter()
            .all(|item| item.state == ItemState::Completed)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SettingsDto;
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
    fn a_transition_to_completed_notifies_once_and_a_one_item_package_adds_nothing() {
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
    fn each_notification_has_its_title_body_and_route() {
        let cases = [
            (
                Notification::Finished {
                    filename: "a.bin".into(),
                },
                ("Download finished", "a.bin"),
                Route::Completed,
            ),
            (
                Notification::PackageFinished {
                    name: "Pack".into(),
                },
                ("Package finished", "Pack"),
                Route::Completed,
            ),
            (
                Notification::Failed {
                    filename: "a.bin".into(),
                    reason: "404".into(),
                },
                ("Download failed", "a.bin: 404"),
                Route::Downloads,
            ),
            (
                Notification::LinksGrabbed { count: 3 },
                (
                    "3 links grabbed",
                    "Waiting for confirmation in the Linkgrabber",
                ),
                Route::Linkgrabber,
            ),
        ];
        for (notification, (title, body), route) in cases {
            assert_eq!(
                notification.title_body(),
                (title.to_owned(), body.to_owned())
            );
            assert_eq!(notification.route(), route);
        }
        assert_eq!(Route::Downloads.as_str(), "downloads");
        assert_eq!(Route::Completed.as_str(), "completed");
        assert_eq!(Route::Linkgrabber.as_str(), "linkgrabber");
    }

    #[test]
    fn each_notification_obeys_its_own_toggle() {
        let all_on = SettingsDto::default();
        let finished = Notification::Finished {
            filename: "a".into(),
        };
        let package = Notification::PackageFinished { name: "p".into() };
        let failed = Notification::Failed {
            filename: "a".into(),
            reason: "r".into(),
        };
        let grabbed = Notification::LinksGrabbed { count: 2 };
        for n in [&finished, &package, &failed, &grabbed] {
            assert!(n.enabled(&all_on));
        }
        let off = |change: fn(&mut SettingsDto)| {
            let mut s = SettingsDto::default();
            change(&mut s);
            s
        };
        assert!(!finished.enabled(&off(|s| s.notify_download_finished = false)));
        assert!(!package.enabled(&off(|s| s.notify_package_finished = false)));
        assert!(!failed.enabled(&off(|s| s.notify_download_failed = false)));
        assert!(!grabbed.enabled(&off(|s| s.notify_links_grabbed = false)));
        // A toggle only silences its own type.
        assert!(finished.enabled(&off(|s| s.notify_download_failed = false)));
        assert!(!Notification::LinksGrabbed { count: 0 }.enabled(&all_on));
    }

    #[test]
    fn a_package_whose_items_all_complete_notifies_once_after_the_items() {
        let (p, a, b) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![package(
                p,
                1,
                vec![
                    item(a, "a.bin", ItemState::Completed),
                    item(b, "b.bin", ItemState::Running),
                ],
            )],
        );
        let new = snapshot(
            2,
            vec![package(
                p,
                2,
                vec![
                    item(a, "a.bin", ItemState::Completed),
                    item(b, "b.bin", ItemState::Completed),
                ],
            )],
        );
        assert_eq!(
            notifications_between(&old, &new),
            vec![
                Notification::Finished {
                    filename: "b.bin".into()
                },
                Notification::PackageFinished { name: "P".into() },
            ]
        );
        // Already finished before: a revision bump alone is not a transition.
        let later = snapshot(
            3,
            vec![package(
                p,
                3,
                vec![
                    item(a, "a.bin", ItemState::Completed),
                    item(b, "b.bin", ItemState::Completed),
                ],
            )],
        );
        assert!(notifications_between(&new, &later).is_empty());
    }

    #[test]
    fn a_package_emptied_of_its_finished_items_does_not_read_as_finished() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(
            1,
            vec![package(p, 1, vec![item(i, "a.bin", ItemState::Completed)])],
        );
        let new = snapshot(2, vec![package(p, 2, vec![])]);
        assert!(notifications_between(&old, &new).is_empty());
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
