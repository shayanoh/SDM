use sdm_core::{ConnectionAllocator, ConnectionBudget, ConnectionDemand};
use uuid::Uuid;

fn demand(host: &str, desired_segments: usize) -> ConnectionDemand {
    ConnectionDemand::new(Uuid::new_v4(), host, desired_segments)
}

#[test]
fn allocate_within_budget_returns_requested_counts() {
    let a = demand("a.com", 4);
    let b = demand("b.com", 4);
    let allocated =
        ConnectionAllocator::allocate(&[a.clone(), b.clone()], &ConnectionBudget::new(32, 8));
    assert_eq!(allocated.get(&a.id), Some(&4));
    assert_eq!(allocated.get(&b.id), Some(&4));
}

#[test]
fn allocate_shrinks_the_largest_pool_first_under_the_global_cap() {
    let small = demand("a.com", 2);
    let large = demand("b.com", 8);
    let allocated = ConnectionAllocator::allocate(
        &[small.clone(), large.clone()],
        &ConnectionBudget::new(6, 8),
    );
    assert_eq!(allocated.get(&small.id), Some(&2));
    assert_eq!(allocated.get(&large.id), Some(&4));
}

#[test]
fn allocate_applies_the_per_host_cap_independently_of_the_global_cap() {
    let d = demand("a.com", 8);
    let allocated =
        ConnectionAllocator::allocate(std::slice::from_ref(&d), &ConnectionBudget::new(32, 3));
    assert_eq!(allocated.get(&d.id), Some(&3));
}

#[test]
fn allocate_never_shrinks_a_pool_below_one() {
    let demands: Vec<ConnectionDemand> = (0..5).map(|_| demand("a.com", 1)).collect();
    let allocated = ConnectionAllocator::allocate(&demands, &ConnectionBudget::new(2, 8));
    assert!(
        demands
            .iter()
            .all(|d| allocated.get(&d.id).copied().unwrap_or(0) == 1)
    );
}

#[test]
fn allocate_shrinks_only_the_over_budget_host_leaving_others_untouched() {
    let busy = demand("busy.com", 10);
    let quiet = demand("quiet.com", 4);
    let allocated = ConnectionAllocator::allocate(
        &[busy.clone(), quiet.clone()],
        &ConnectionBudget::new(32, 5),
    );
    assert_eq!(allocated.get(&busy.id), Some(&5));
    assert_eq!(allocated.get(&quiet.id), Some(&4));
}

#[test]
fn allocate_of_empty_demands_is_empty() {
    assert!(ConnectionAllocator::allocate(&[], &ConnectionBudget::new(8, 4)).is_empty());
}
