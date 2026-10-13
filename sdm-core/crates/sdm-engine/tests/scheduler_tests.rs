use std::collections::HashSet;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, Priority, SourceUrl};
use sdm_engine::{Scheduler, SchedulerInput};
use uuid::Uuid;

/// Mirrors the Swift `item(...)` helper: 1000 bytes, resumable, queued,
/// enabled and at position 0 unless overridden.
struct ItemSpec {
    name: &'static str,
    priority: Option<Priority>,
    position: i64,
    enabled: bool,
    resumable: Option<bool>,
    state: ItemState,
}

fn spec(name: &'static str) -> ItemSpec {
    ItemSpec {
        name,
        priority: None,
        position: 0,
        enabled: true,
        resumable: Some(true),
        state: ItemState::Queued,
    }
}

impl ItemSpec {
    fn priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }
    fn position(mut self, position: i64) -> Self {
        self.position = position;
        self
    }
    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    fn resumable(mut self, resumable: bool) -> Self {
        self.resumable = Some(resumable);
        self
    }
    fn state(mut self, state: ItemState) -> Self {
        self.state = state;
        self
    }
    fn build(self) -> DownloadItem {
        let url = SourceUrl::new(format!("https://example.com/{}", self.name)).unwrap();
        let mut item = DownloadItem::http(url, self.name)
            .with_total_bytes(1000)
            .with_state(self.state)
            .with_enabled(self.enabled)
            .with_position(self.position);
        item.set_is_resumable(self.resumable);
        if let Some(priority) = self.priority {
            item = item.with_priority(priority);
        }
        item
    }
}

fn package(
    name: &str,
    priority: Priority,
    position: i64,
    items: Vec<DownloadItem>,
) -> DownloadPackage {
    DownloadPackage::new(name)
        .with_items(items)
        .with_priority(priority)
        .with_position(position)
}

fn input(
    packages: Vec<DownloadPackage>,
    running_now: &[Uuid],
    started_recently: &[Uuid],
    max_concurrent: usize,
) -> SchedulerInput {
    SchedulerInput::new(
        packages,
        running_now.iter().copied().collect(),
        started_recently.iter().copied().collect(),
        max_concurrent,
    )
}

fn set(ids: &[Uuid]) -> HashSet<Uuid> {
    ids.iter().copied().collect()
}

#[test]
fn running_set_never_exceeds_max_concurrent() {
    let items: Vec<_> = (0..10).map(|i| spec("f.bin").position(i).build()).collect();
    let input = input(vec![package("p", Priority::Normal, 0, items)], &[], &[], 3);
    assert_eq!(Scheduler::desired_running_set(input).len(), 3);
}

#[test]
fn higher_priority_wins_regardless_of_position() {
    let low = spec("low.bin").position(0).build();
    let high = spec("high.bin")
        .priority(Priority::Highest)
        .position(9)
        .build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![low.clone(), high.clone()],
        )],
        &[],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[high.id]));
}

#[test]
fn package_priority_lifts_its_items() {
    let normal = spec("a.bin").position(0).build();
    let lifted = spec("b.bin").position(0).build();
    let input = input(
        vec![
            package("normal", Priority::Normal, 0, vec![normal.clone()]),
            package("urgent", Priority::Highest, 1, vec![lifted.clone()]),
        ],
        &[],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[lifted.id]));
}

#[test]
fn item_priority_overrides_its_package() {
    let pinned = spec("pinned.bin")
        .priority(Priority::Highest)
        .position(5)
        .build();
    let sibling = spec("other.bin").position(0).build();
    let input = input(
        vec![package(
            "low",
            Priority::Lowest,
            0,
            vec![sibling.clone(), pinned.clone()],
        )],
        &[],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[pinned.id]));
}

#[test]
fn disabled_items_are_never_scheduled() {
    let disabled = spec("off.bin").position(0).enabled(false).build();
    let enabled = spec("on.bin").position(1).build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![disabled.clone(), enabled.clone()],
        )],
        &[],
        &[],
        5,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[enabled.id]));
}

#[test]
fn completed_and_failed_items_are_never_scheduled() {
    let done = spec("done.bin")
        .position(0)
        .state(ItemState::Completed)
        .build();
    let failed = spec("bad.bin")
        .position(1)
        .state(ItemState::Failed {
            reason: "404".to_string(),
        })
        .build();
    let ready = spec("go.bin").position(2).build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![done.clone(), failed.clone(), ready.clone()],
        )],
        &[],
        &[],
        5,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[ready.id]));
}

#[test]
fn running_non_resumable_item_keeps_its_slot_against_higher_priority() {
    let stubborn = spec("noresume.bin")
        .priority(Priority::Lowest)
        .position(9)
        .resumable(false)
        .build();
    let urgent = spec("urgent.bin")
        .priority(Priority::Highest)
        .position(0)
        .build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![stubborn.clone(), urgent.clone()],
        )],
        &[stubborn.id],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[stubborn.id]));
}

#[test]
fn non_resumable_item_that_is_not_running_has_no_special_claim() {
    let stubborn = spec("noresume.bin")
        .priority(Priority::Lowest)
        .position(9)
        .resumable(false)
        .build();
    let urgent = spec("urgent.bin")
        .priority(Priority::Highest)
        .position(0)
        .build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![stubborn.clone(), urgent.clone()],
        )],
        &[],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[urgent.id]));
}

#[test]
fn hysteresis_protects_recently_started_items() {
    let fresh = spec("fresh.bin")
        .priority(Priority::Lowest)
        .position(9)
        .build();
    let urgent = spec("urgent.bin")
        .priority(Priority::Highest)
        .position(0)
        .build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![fresh.clone(), urgent.clone()],
        )],
        &[fresh.id],
        &[fresh.id],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[fresh.id]));
}

#[test]
fn settled_running_item_is_preempted_by_higher_priority() {
    let settled = spec("settled.bin")
        .priority(Priority::Lowest)
        .position(9)
        .build();
    let urgent = spec("urgent.bin")
        .priority(Priority::Highest)
        .position(0)
        .build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![settled.clone(), urgent.clone()],
        )],
        &[settled.id],
        &[],
        1,
    );
    assert_eq!(Scheduler::desired_running_set(input), set(&[urgent.id]));
}

#[test]
fn reservations_are_capped_by_max_concurrent() {
    let a = spec("a.bin").position(0).resumable(false).build();
    let b = spec("b.bin").position(1).resumable(false).build();
    let c = spec("c.bin").position(2).resumable(false).build();
    let input = input(
        vec![package(
            "p",
            Priority::Normal,
            0,
            vec![a.clone(), b.clone(), c.clone()],
        )],
        &[a.id, b.id, c.id],
        &[],
        2,
    );
    assert_eq!(Scheduler::desired_running_set(input).len(), 2);
}

#[test]
fn rank_orders_by_priority_then_package_then_position() {
    let packages = vec![
        package(
            "second",
            Priority::Normal,
            1,
            vec![
                spec("b1.bin").position(1).build(),
                spec("b0.bin").position(0).build(),
            ],
        ),
        package(
            "first",
            Priority::Normal,
            0,
            vec![spec("a0.bin").position(0).build()],
        ),
        package(
            "urgent",
            Priority::High,
            2,
            vec![spec("z.bin").position(0).build()],
        ),
    ];
    let names: Vec<String> = Scheduler::rank(&packages)
        .iter()
        .map(|item| item.filename().to_string())
        .collect();
    assert_eq!(names, ["z.bin", "a0.bin", "b0.bin", "b1.bin"]);
}
