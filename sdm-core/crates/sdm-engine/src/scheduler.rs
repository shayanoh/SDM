use std::collections::HashSet;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, Priority};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SchedulerInput {
    pub packages: Vec<DownloadPackage>,
    /// Items the engine currently has running.
    pub running_now: HashSet<Uuid>,
    /// Items started within the hysteresis window; protected from preemption.
    pub started_recently: HashSet<Uuid>,
    pub max_concurrent: usize,
}

impl SchedulerInput {
    pub fn new(
        packages: Vec<DownloadPackage>,
        running_now: HashSet<Uuid>,
        started_recently: HashSet<Uuid>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            packages,
            running_now,
            started_recently,
            max_concurrent,
        }
    }
}

/// The scheduling policy, expressed as a pure function so every behavior in
/// spec §6 is table-testable. Re-evaluated on every change rather than
/// maintained as a queue.
pub enum Scheduler {}

impl Scheduler {
    /// Items eligible to run, ordered best-first.
    ///
    /// Sort key: effective priority descending, then package position, then
    /// item position. The sort is stable, so equal keys keep list order.
    pub fn rank(packages: &[DownloadPackage]) -> Vec<DownloadItem> {
        let mut scored: Vec<(&DownloadItem, Priority, i64)> = Vec::new();
        for package in packages {
            for item in package.items.iter().filter(|item| is_eligible(item)) {
                scored.push((item, package.effective_priority(item), package.position));
            }
        }
        scored.sort_by(|lhs, rhs| {
            rhs.1
                .cmp(&lhs.1)
                .then(lhs.2.cmp(&rhs.2))
                .then(lhs.0.position.cmp(&rhs.0.position))
        });
        scored
            .into_iter()
            .map(|(item, _, _)| item.clone())
            .collect()
    }

    /// The set of item IDs that should be running right now.
    ///
    /// Slots are allocated in three passes: running non-resumable items (which
    /// cannot be preempted), then running items still inside the hysteresis
    /// window, then the highest-ranked remainder.
    pub fn desired_running_set(input: SchedulerInput) -> HashSet<Uuid> {
        let ranked = Self::rank(&input.packages);
        let mut slots = input.max_concurrent;
        let mut desired: HashSet<Uuid> = HashSet::new();

        let mut reserve = |candidates: &mut dyn Iterator<Item = &DownloadItem>| {
            for candidate in candidates {
                if slots > 0 && !desired.contains(&candidate.id) {
                    desired.insert(candidate.id);
                    slots -= 1;
                }
            }
        };

        // Pass 1: running non-resumable items keep their slots unconditionally
        // — preempting one throws away every byte it has, since it cannot be
        // resumed.
        //
        // Keyed on `== Some(false)`, not "not resumable": an item that has not
        // been probed yet is `None`, and treating unknown as non-resumable
        // would hand an unconditional claim to every item the engine has just
        // started, making the list effectively unpreemptible.
        reserve(&mut ranked.iter().filter(|item| {
            input.running_now.contains(&item.id) && item.is_resumable() == Some(false)
        }));

        // Pass 2: recently started items are protected from drag-induced churn.
        reserve(&mut ranked.iter().filter(|item| {
            input.running_now.contains(&item.id) && input.started_recently.contains(&item.id)
        }));

        // Pass 3: fill whatever remains by rank.
        reserve(&mut ranked.iter());

        // Every pass draws from `ranked`, and `ranked` contains only eligible
        // items, so no ineligible id can be in `desired` to begin with.
        desired
    }
}

fn is_eligible(item: &DownloadItem) -> bool {
    if !item.is_enabled {
        return false;
    }
    match item.state {
        ItemState::Queued | ItemState::Running => true,
        ItemState::Stopped | ItemState::Completed | ItemState::Failed { .. } => false,
    }
}
