//! Turns successive engine snapshots into deltas.
//!
//! It remembers what it last sent. A package whose `revision` matches the one
//! last sent is skipped without looking at its items (the engine bumps a
//! package's revision whenever anything inside it changed), which keeps an
//! idle or mostly idle list cheap.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use sdm_engine::EngineSnapshot;
use uuid::Uuid;

use crate::convert;
use crate::protocol::*;

pub(crate) struct Delta {
    pub structure: Option<Structure>,
    pub items: Vec<ItemFigures>,
    pub packages: Vec<PackageFigures>,
    pub stats: Option<EngineStats>,
}

#[derive(Default)]
pub(crate) struct Differ {
    structure: Vec<StructurePackage>,
    revisions: HashMap<Uuid, u64>,
    items: HashMap<String, ItemFigures>,
    packages: HashMap<String, PackageFigures>,
    stats: Option<EngineStats>,
    download_folder: PathBuf,
}

impl Differ {
    /// Forgets everything sent, so the next diff is a full one. The download
    /// folder is configuration, not sent state, so it stays.
    pub(crate) fn reset(&mut self) {
        let download_folder = std::mem::take(&mut self.download_folder);
        *self = Self {
            download_folder,
            ..Self::default()
        };
    }

    /// Sets the folder package folders are built from. Returns whether it
    /// changed; a change forgets every package revision, so the next diff
    /// re-evaluates (and re-sends the structure of) every package.
    pub(crate) fn set_download_folder(&mut self, folder: PathBuf) -> bool {
        if self.download_folder == folder {
            return false;
        }
        self.download_folder = folder;
        self.revisions.clear();
        true
    }

    /// The change since the last call, or `None` when nothing changed.
    pub(crate) fn diff(&mut self, snapshot: &EngineSnapshot) -> Option<Delta> {
        self.diff_inner(snapshot, false)
    }

    /// Everything, regardless of what was sent before; afterwards the snapshot
    /// counts as sent.
    pub(crate) fn full(&mut self, snapshot: &EngineSnapshot) -> Delta {
        self.reset();
        self.diff_inner(snapshot, true)
            .expect("a forced diff always produces a delta")
    }

    fn diff_inner(&mut self, snapshot: &EngineSnapshot, force: bool) -> Option<Delta> {
        let old_order: Vec<String> = self.structure.iter().map(|p| p.id.clone()).collect();
        let mut old_by_id: HashMap<String, StructurePackage> = std::mem::take(&mut self.structure)
            .into_iter()
            .map(|package| (package.id.clone(), package))
            .collect();

        let mut new_structure = Vec::with_capacity(snapshot.packages.len());
        let mut items_out = Vec::new();
        let mut packages_out = Vec::new();
        let mut structure_changed = force;

        for package in &snapshot.packages {
            let package_id = package.id.to_string();
            let old = old_by_id.remove(&package_id);
            let revision_unchanged =
                !force && self.revisions.get(&package.id) == Some(&package.revision);

            let entry = match (revision_unchanged, old) {
                (true, Some(old)) => old,
                (_, old) => {
                    let fresh = convert::structure_package(package, &self.download_folder);
                    if old.as_ref() != Some(&fresh) {
                        structure_changed = true;
                    }
                    for item in &package.items {
                        let figures = convert::item_figures(item);
                        if self.items.get(&figures.id) != Some(&figures) {
                            self.items.insert(figures.id.clone(), figures.clone());
                            items_out.push(figures);
                        }
                    }
                    let figures = convert::package_figures(package);
                    if self.packages.get(&figures.id) != Some(&figures) {
                        self.packages.insert(figures.id.clone(), figures.clone());
                        packages_out.push(figures);
                    }
                    self.revisions.insert(package.id, package.revision);
                    fresh
                }
            };
            new_structure.push(entry);
        }

        if !old_by_id.is_empty() {
            structure_changed = true;
            for (id, removed) in &old_by_id {
                self.packages.remove(id);
                for item in &removed.items {
                    self.items.remove(&item.id);
                }
            }
            let live: HashSet<Uuid> = snapshot.packages.iter().map(|p| p.id).collect();
            self.revisions.retain(|id, _| live.contains(id));
        }
        let new_order: Vec<&String> = new_structure.iter().map(|p| &p.id).collect();
        if old_order.iter().ne(new_order.iter().copied()) {
            structure_changed = true;
        }
        if structure_changed {
            // Drop figures of items that left a package that stayed.
            let live_items: HashSet<&str> = new_structure
                .iter()
                .flat_map(|p| p.items.iter().map(|i| i.id.as_str()))
                .collect();
            self.items.retain(|id, _| live_items.contains(id.as_str()));
        }

        let stats = convert::engine_stats(snapshot);
        let stats_changed = force || self.stats.as_ref() != Some(&stats);
        if stats_changed {
            self.stats = Some(stats.clone());
        }

        let structure = structure_changed.then(|| Structure {
            packages: new_structure.clone(),
        });
        self.structure = new_structure;

        if structure.is_none() && items_out.is_empty() && packages_out.is_empty() && !stats_changed
        {
            return None;
        }
        Some(Delta {
            structure,
            items: items_out,
            packages: packages_out,
            stats: stats_changed.then_some(stats),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    struct Fixture {
        package_id: Uuid,
        item_ids: Vec<Uuid>,
    }

    fn item(id: Uuid, state: ItemState, done: i64, speed: f64) -> ItemSnapshot {
        let mut completed = RangeSet::new();
        if done > 0 {
            completed.insert(ByteRange::new(0, done));
        }
        ItemSnapshot::new(
            id,
            SourceUrl::new("https://example.com/f").unwrap(),
            "f.bin",
            Some(1000),
            completed,
            state,
            true,
            Some(true),
            1,
            4,
            speed,
            vec![speed],
        )
    }

    fn snapshot(
        fixture: &Fixture,
        revision: u64,
        generation: u64,
        items: Vec<ItemSnapshot>,
    ) -> EngineSnapshot {
        let package = PackageSnapshot::new(
            fixture.package_id,
            "Pack",
            Priority::Normal,
            items,
            0.0,
            revision,
        );
        EngineSnapshot::new(vec![package], 0.0, vec![], generation)
    }

    fn fixture(count: usize) -> Fixture {
        Fixture {
            package_id: Uuid::new_v4(),
            item_ids: (0..count).map(|_| Uuid::new_v4()).collect(),
        }
    }

    #[test]
    fn the_first_diff_sends_everything() {
        let f = fixture(2);
        let snap = snapshot(
            &f,
            1,
            1,
            vec![
                item(f.item_ids[0], ItemState::Running, 10, 5.0),
                item(f.item_ids[1], ItemState::Queued, 0, 0.0),
            ],
        );
        let mut differ = Differ::default();
        let delta = differ.diff(&snap).expect("first diff is never empty");
        let structure = delta.structure.expect("structure sent first time");
        assert_eq!(structure.packages[0].items.len(), 2);
        assert_eq!(delta.items.len(), 2);
        assert_eq!(delta.packages.len(), 1);
        assert!(delta.stats.is_some());
    }

    #[test]
    fn an_identical_snapshot_produces_no_delta() {
        let f = fixture(1);
        let snap = snapshot(
            &f,
            1,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        );
        let mut differ = Differ::default();
        differ.diff(&snap);
        assert!(differ.diff(&snap).is_none());
    }

    #[test]
    fn a_progress_change_sends_figures_only() {
        let f = fixture(2);
        let mut differ = Differ::default();
        differ.diff(&snapshot(
            &f,
            1,
            1,
            vec![
                item(f.item_ids[0], ItemState::Running, 10, 5.0),
                item(f.item_ids[1], ItemState::Running, 10, 5.0),
            ],
        ));
        let delta = differ
            .diff(&snapshot(
                &f,
                2,
                2,
                vec![
                    item(f.item_ids[0], ItemState::Running, 50, 5.0),
                    item(f.item_ids[1], ItemState::Running, 10, 5.0),
                ],
            ))
            .expect("progress changed");
        assert!(delta.structure.is_none(), "shape did not change");
        assert_eq!(delta.items.len(), 1, "only the changed item is sent");
        assert_eq!(delta.items[0].id, f.item_ids[0].to_string());
        assert_eq!(delta.items[0].completed_bytes, 50.0);
    }

    #[test]
    fn a_state_change_sends_the_structure() {
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(
            &f,
            1,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        ));
        let delta = differ
            .diff(&snapshot(
                &f,
                2,
                2,
                vec![item(f.item_ids[0], ItemState::Completed, 1000, 0.0)],
            ))
            .unwrap();
        let structure = delta.structure.expect("state is structural");
        assert_eq!(
            structure.packages[0].items[0].state,
            ItemStateDto::Completed
        );
    }

    #[test]
    fn an_unchanged_revision_is_skipped_without_comparing_items() {
        // Same revision but (deliberately) different item figures (the speed:
        // the engine-wide figures are derived from the items' bytes, so changing
        // those would legitimately change `stats`): the differ trusts
        // the engine's revision, exactly as the SwiftUI app does.
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(
            &f,
            7,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        ));
        assert!(
            differ
                .diff(&snapshot(
                    &f,
                    7,
                    2,
                    vec![item(f.item_ids[0], ItemState::Running, 10, 99.0)]
                ))
                .is_none()
        );
    }

    #[test]
    fn a_removed_item_is_dropped_from_the_structure() {
        let f = fixture(2);
        let mut differ = Differ::default();
        differ.diff(&snapshot(
            &f,
            1,
            1,
            vec![
                item(f.item_ids[0], ItemState::Running, 10, 5.0),
                item(f.item_ids[1], ItemState::Running, 10, 5.0),
            ],
        ));
        let delta = differ
            .diff(&snapshot(
                &f,
                2,
                2,
                vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
            ))
            .unwrap();
        assert_eq!(delta.structure.unwrap().packages[0].items.len(), 1);
        // The removed item's figures are forgotten: re-adding it later sends them again.
        let again = differ
            .diff(&snapshot(
                &f,
                3,
                3,
                vec![
                    item(f.item_ids[0], ItemState::Running, 10, 5.0),
                    item(f.item_ids[1], ItemState::Running, 10, 5.0),
                ],
            ))
            .unwrap();
        assert!(
            again
                .items
                .iter()
                .any(|i| i.id == f.item_ids[1].to_string())
        );
    }

    #[test]
    fn a_removed_package_is_dropped() {
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(
            &f,
            1,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        ));
        let empty = EngineSnapshot::new(vec![], 0.0, vec![], 2);
        let delta = differ.diff(&empty).expect("package removal is a change");
        assert!(delta.structure.unwrap().packages.is_empty());
    }

    #[test]
    fn full_always_returns_everything_even_for_an_empty_engine() {
        let mut differ = Differ::default();
        let delta = differ.full(&EngineSnapshot::new(vec![], 0.0, vec![], 1));
        assert!(delta.structure.is_some());
        assert!(delta.stats.is_some());
    }

    #[test]
    fn full_resets_what_was_sent() {
        let f = fixture(1);
        let snap = snapshot(
            &f,
            1,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        );
        let mut differ = Differ::default();
        differ.diff(&snap);
        let delta = differ.full(&snap);
        assert_eq!(delta.items.len(), 1);
        assert!(
            differ.diff(&snap).is_none(),
            "after full the snapshot counts as sent"
        );
    }

    #[test]
    fn structure_carries_folder_and_assembly() {
        let f = fixture(1);
        let muxed = item(f.item_ids[0], ItemState::Running, 10, 5.0)
            .with_assembly(sdm_core::Assembly::Mux)
            .with_remaining_attempts(2);
        let mut differ = Differ::default();
        differ.set_download_folder(std::path::PathBuf::from("dl"));
        let delta = differ
            .diff(&snapshot(&f, 1, 1, vec![muxed]))
            .expect("first diff");
        let structure = delta.structure.expect("structure sent first time");
        assert_eq!(
            structure.packages[0].folder,
            std::path::Path::new("dl").join("Pack").to_string_lossy()
        );
        assert_eq!(structure.packages[0].items[0].assembly, AssemblyDto::Mux);
        assert_eq!(delta.items[0].remaining_attempts, Some(2));
    }

    #[test]
    fn changing_the_download_folder_changes_the_structure() {
        let f = fixture(1);
        let snap = snapshot(
            &f,
            1,
            1,
            vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)],
        );
        let mut differ = Differ::default();
        assert!(differ.set_download_folder(std::path::PathBuf::from("a")));
        differ.diff(&snap);
        assert!(
            !differ.set_download_folder(std::path::PathBuf::from("a")),
            "the same folder is not a change"
        );
        assert!(differ.set_download_folder(std::path::PathBuf::from("b")));
        let delta = differ
            .diff(&snap)
            .expect("a new folder must re-send the structure even at the same revision");
        assert_eq!(
            delta.structure.unwrap().packages[0].folder,
            std::path::Path::new("b").join("Pack").to_string_lossy()
        );
    }

    #[test]
    fn reordering_packages_sends_the_structure() {
        let a = fixture(1);
        let b = fixture(1);
        let pa = PackageSnapshot::new(
            a.package_id,
            "A",
            Priority::Normal,
            vec![item(a.item_ids[0], ItemState::Running, 1, 1.0)],
            0.0,
            1,
        );
        let pb = PackageSnapshot::new(
            b.package_id,
            "B",
            Priority::Normal,
            vec![item(b.item_ids[0], ItemState::Running, 1, 1.0)],
            0.0,
            1,
        );
        let mut differ = Differ::default();
        differ.diff(&EngineSnapshot::new(
            vec![pa.clone(), pb.clone()],
            0.0,
            vec![],
            1,
        ));
        let delta = differ
            .diff(&EngineSnapshot::new(vec![pb, pa], 0.0, vec![], 2))
            .expect("order changed");
        let order: Vec<String> = delta
            .structure
            .unwrap()
            .packages
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(order, ["B", "A"]);
    }
}
