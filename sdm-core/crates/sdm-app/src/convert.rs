//! Engine snapshot types -> wire types.

use sdm_core::ItemState;
use sdm_engine::{EngineSnapshot, ItemSnapshot, PackageSnapshot};

use crate::protocol::*;

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn state_dto(state: &ItemState) -> ItemStateDto {
    match state {
        ItemState::Stopped => ItemStateDto::Stopped,
        ItemState::Queued => ItemStateDto::Queued,
        ItemState::Running => ItemStateDto::Running,
        ItemState::Completed => ItemStateDto::Completed,
        ItemState::Failed { reason } => ItemStateDto::Failed {
            reason: reason.clone(),
        },
    }
}

pub(crate) fn structure_package(package: &PackageSnapshot) -> StructurePackage {
    StructurePackage {
        id: package.id.to_string(),
        name: package.name.clone(),
        priority: package.priority as u32,
        items: package
            .items
            .iter()
            .map(|item| StructureItem {
                id: item.id.to_string(),
                url: item.url.as_str().to_owned(),
                filename: item.filename.clone(),
                state: state_dto(&item.state),
                is_enabled: item.is_enabled,
                is_resumable: item.is_resumable,
                is_assembling: item.is_assembling,
                file_missing: item.file_missing,
                metadata: item.metadata.clone(),
            })
            .collect(),
    }
}

pub(crate) fn item_figures(item: &ItemSnapshot) -> ItemFigures {
    ItemFigures {
        id: item.id.to_string(),
        completed: item
            .completed
            .ranges()
            .iter()
            .map(|range| RangeDto {
                start: range.start as f64,
                end: range.end as f64,
            })
            .collect(),
        completed_bytes: item.completed.total_bytes() as f64,
        total_bytes: item.total_bytes.map(|bytes| bytes as f64),
        active_segments: count(item.active_segments),
        configured_segments: count(item.configured_segments),
        bytes_per_second: item.bytes_per_second,
        speed_history: item.speed_history.clone(),
        retry_hold_seconds: item.retry_hold_seconds.map(count),
        failed_attempt_count: item.failed_attempt_count.map(count),
        last_failure_reason: item.last_failure_reason.clone(),
        checkpoint_failure: item.checkpoint_failure.clone(),
    }
}

pub(crate) fn package_figures(package: &PackageSnapshot) -> PackageFigures {
    let aggregate = &package.aggregate;
    PackageFigures {
        id: package.id.to_string(),
        completed_bytes: aggregate.completed_bytes as f64,
        total_bytes: aggregate.total_bytes as f64,
        bytes_per_second: aggregate.bytes_per_second,
        speed_history: aggregate.speed_history.clone(),
        eta_seconds: aggregate.eta,
    }
}

pub(crate) fn engine_stats(snapshot: &EngineSnapshot) -> EngineStats {
    let figures = &snapshot.figures;
    EngineStats {
        bytes_per_second: snapshot.global_bytes_per_second,
        history: snapshot.global_history.clone(),
        running_count: count(figures.running_count),
        queued_count: count(figures.queued_count),
        overall_fraction: figures.overall_fraction,
        ring_completed_bytes: figures.ring_completed_bytes as f64,
        ring_total_bytes: figures.ring_total_bytes as f64,
        waiting_item_count: count(figures.waiting_item_count),
        waiting_package_count: count(figures.waiting_package_count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    fn item(id: Uuid, state: ItemState) -> ItemSnapshot {
        let mut completed = RangeSet::new();
        completed.insert(ByteRange::new(0, 100));
        completed.insert(ByteRange::new(200, 250));
        ItemSnapshot::new(
            id,
            SourceUrl::new("https://example.com/a.bin").unwrap(),
            "a.bin",
            Some(1000),
            completed,
            state,
            true,
            Some(true),
            2,
            4,
            512.0,
            vec![1.0, 2.0],
        )
    }

    #[test]
    fn item_figures_carry_ranges_and_counts() {
        let id = Uuid::new_v4();
        let figures = item_figures(&item(id, ItemState::Running));
        assert_eq!(figures.id, id.to_string());
        assert_eq!(figures.completed_bytes, 150.0);
        assert_eq!(figures.total_bytes, Some(1000.0));
        assert_eq!(figures.completed.len(), 2);
        assert_eq!(
            figures.completed[1],
            RangeDto {
                start: 200.0,
                end: 250.0
            }
        );
        assert_eq!(
            (figures.active_segments, figures.configured_segments),
            (2, 4)
        );
        assert_eq!(figures.speed_history, vec![1.0, 2.0]);
    }

    #[test]
    fn a_failed_state_keeps_its_reason() {
        let id = Uuid::new_v4();
        let package = PackageSnapshot::new(
            Uuid::new_v4(),
            "Pack",
            Priority::High,
            vec![item(
                id,
                ItemState::Failed {
                    reason: "disk full".into(),
                },
            )],
            0.0,
            1,
        );
        let structure = structure_package(&package);
        assert_eq!(structure.priority, 3);
        assert_eq!(
            structure.items[0].state,
            ItemStateDto::Failed {
                reason: "disk full".into()
            }
        );
    }

    #[test]
    fn package_figures_use_the_aggregate() {
        let package = PackageSnapshot::new(
            Uuid::new_v4(),
            "Pack",
            Priority::Normal,
            vec![item(Uuid::new_v4(), ItemState::Running)],
            0.0,
            1,
        );
        let figures = package_figures(&package);
        assert_eq!(figures.completed_bytes, 150.0);
        assert_eq!(figures.total_bytes, 1000.0);
        assert_eq!(figures.bytes_per_second, 512.0);
    }
}
