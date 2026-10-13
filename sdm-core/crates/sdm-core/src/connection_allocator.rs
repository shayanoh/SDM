use std::collections::HashMap;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionBudget {
    pub global: usize,
    pub per_host: usize,
}

impl ConnectionBudget {
    /// # Panics
    ///
    /// Panics unless `global >= 1` and `per_host >= 1`.
    pub fn new(global: usize, per_host: usize) -> Self {
        assert!(global >= 1, "global must be at least 1");
        assert!(per_host >= 1, "perHost must be at least 1");
        Self { global, per_host }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionDemand {
    pub id: Uuid,
    pub host: String,
    pub desired_segments: usize,
}

impl ConnectionDemand {
    /// # Panics
    ///
    /// Panics unless `desired_segments >= 1`.
    pub fn new(id: Uuid, host: impl Into<String>, desired_segments: usize) -> Self {
        assert!(desired_segments >= 1, "desiredSegments must be at least 1");
        Self {
            id,
            host: host.into(),
            desired_segments,
        }
    }
}

/// Shrinks per-item worker-pool sizes to fit within a global connection
/// ceiling and a per-host ceiling. Spec §6.4: "worker pools shrink below
/// their configured N when the budget is tight, largest pool yielding
/// first."
///
/// Never shrinks a pool below 1: if every pool is already at the floor and
/// the total still exceeds `budget.global`, the ceiling is a soft target
/// from here on — killing an item outright to enforce it is the scheduler's
/// job (`maxConcurrent`), not this one's.
pub enum ConnectionAllocator {}

impl ConnectionAllocator {
    pub fn allocate(
        demands: &[ConnectionDemand],
        budget: &ConnectionBudget,
    ) -> HashMap<Uuid, usize> {
        let mut allocated: HashMap<Uuid, usize> = HashMap::new();
        for demand in demands {
            allocated.insert(demand.id, demand.desired_segments);
        }

        fn get(allocated: &HashMap<Uuid, usize>, id: Uuid) -> usize {
            allocated.get(&id).copied().unwrap_or(0)
        }
        fn total_for_host(
            demands: &[ConnectionDemand],
            allocated: &HashMap<Uuid, usize>,
            host: &str,
        ) -> usize {
            demands
                .iter()
                .filter(|d| d.host == host)
                .map(|d| get(allocated, d.id))
                .sum()
        }
        // Swift: `.filter { allocated > 1 }.max(by: { allocated($0) < allocated($1) })`.
        // Swift's `max(by:)` keeps the first of equal maxima; Rust's `max_by`
        // keeps the last, so fold explicitly.
        fn shrink_largest<'a>(
            allocated: &mut HashMap<Uuid, usize>,
            candidates: impl Iterator<Item = &'a ConnectionDemand>,
        ) -> bool {
            let mut victim: Option<&ConnectionDemand> = None;
            for candidate in candidates.filter(|d| get(allocated, d.id) > 1) {
                victim = match victim {
                    Some(best) if get(allocated, best.id) >= get(allocated, candidate.id) => {
                        Some(best)
                    }
                    _ => Some(candidate),
                };
            }
            let Some(victim) = victim else { return false };
            let current = allocated.get(&victim.id).copied().unwrap_or(1);
            allocated.insert(victim.id, current - 1);
            true
        }

        // Per-host pass first: each over-budget host is brought into line on
        // its own, independent of every other host. (Swift iterates a `Set`
        // of hosts in arbitrary order; hosts are independent here, so
        // first-appearance order is equivalent.)
        let mut hosts: Vec<&str> = Vec::new();
        for demand in demands {
            if !hosts.contains(&demand.host.as_str()) {
                hosts.push(&demand.host);
            }
        }
        for host in hosts {
            while total_for_host(demands, &allocated, host) > budget.per_host {
                let host_demands = demands.iter().filter(|d| d.host == host);
                if !shrink_largest(&mut allocated, host_demands) {
                    break;
                }
            }
        }

        // Global pass: shrinks the largest remaining pool across every host
        // until the grand total fits.
        while allocated.values().sum::<usize>() > budget.global {
            if !shrink_largest(&mut allocated, demands.iter()) {
                break;
            }
        }

        allocated
    }
}
