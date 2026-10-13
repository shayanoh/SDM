use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use sdm_core::{ByteRange, RangeSet};

fn br(start: i64, end: i64) -> ByteRange {
    ByteRange::new(start, end)
}

#[test]
fn gaps_of_empty_set_is_whole_file() {
    assert_eq!(RangeSet::new().gaps_within(100), [br(0, 100)]);
}

#[test]
fn gaps_between_and_after_ranges() {
    let set = RangeSet::from_ranges(&[br(10, 20), br(40, 50)]);
    assert_eq!(set.gaps_within(100), [br(0, 10), br(20, 40), br(50, 100)]);
}

#[test]
fn no_gaps_when_complete() {
    let set = RangeSet::from_ranges(&[br(0, 100)]);
    assert!(set.gaps_within(100).is_empty());
    assert!(set.is_complete(100));
}

#[test]
fn incomplete_when_any_gap_remains() {
    let set = RangeSet::from_ranges(&[br(0, 99)]);
    assert!(!set.is_complete(100));
}

#[test]
fn first_claim_takes_the_whole_file() {
    // No pre-emptive halving: a fresh claim takes the entire free gap.
    // Splitting only happens later, on demand, when another worker needs
    // work and none is free — see `DownloadTask`'s stealing logic.
    let claim = RangeSet::new().next_claim(1000, &[]);
    assert_eq!(claim, Some(br(0, 1000)));
}

#[test]
fn second_claim_takes_the_whole_remainder_past_a_reserved_range() {
    let claim = RangeSet::new().next_claim(1000, &[br(0, 500)]);
    assert_eq!(claim, Some(br(500, 1000)));
}

#[test]
fn small_gap_is_taken_whole() {
    let claim = RangeSet::new().next_claim(15, &[]);
    assert_eq!(claim, Some(br(0, 15)));
}

#[test]
fn claim_prefers_largest_gap() {
    let set = RangeSet::from_ranges(&[br(100, 200)]);
    let claim = set.next_claim(1000, &[]);
    assert_eq!(claim, Some(br(200, 1000)));
}

#[test]
fn no_claim_when_everything_is_done_or_reserved() {
    let set = RangeSet::from_ranges(&[br(0, 500)]);
    let claim = set.next_claim(1000, &[br(500, 1000)]);
    assert_eq!(claim, None);
}

#[test]
fn claim_skips_fragmented_holes_left_by_retired_workers_preferring_the_lower_start_on_a_tie() {
    let set = RangeSet::from_ranges(&[br(0, 100), br(150, 400), br(450, 1000)]);
    let claim = set.next_claim(1000, &[]);
    assert_eq!(claim, Some(br(100, 150)));
}

fn overlaps(a: ByteRange, b: ByteRange) -> bool {
    a.start < b.end && b.start < a.end
}

/// Beyond the Swift tests. Seeded random interleavings of claim / complete /
/// release (plus a steal-style shrink of a claim, which is what fragments the
/// space) over a 1 MB file. After every step: no two outstanding claims
/// overlap, no claim overlaps a completed range, and completed, claimed and
/// remaining together cover the space exactly once.
#[test]
fn random_claim_complete_release_interleavings_never_double_hand_out_a_range() {
    const TOTAL: i64 = 1_048_576; // 1 MB
    let mut rng = SmallRng::seed_from_u64(0x5D4D_C0DE);
    let mut completed = RangeSet::new();
    let mut claims: Vec<ByteRange> = Vec::new();

    for step in 0..2_000 {
        match rng.random_range(0..4) {
            // Claim.
            0 => {
                if let Some(claim) = completed.next_claim(TOTAL, &claims) {
                    claims.push(claim);
                }
            }
            // Complete a random sub-range of a claim; the rest is released.
            1 if !claims.is_empty() => {
                let claim = claims.swap_remove(rng.random_range(0..claims.len()));
                let a = rng.random_range(claim.start..=claim.end);
                let b = rng.random_range(claim.start..=claim.end);
                completed.insert(ByteRange::new(a.min(b), a.max(b)));
            }
            // Release a whole claim.
            2 if !claims.is_empty() => {
                claims.swap_remove(rng.random_range(0..claims.len()));
            }
            // Shrink a claim to a random prefix, as a stealing worker would.
            3 if !claims.is_empty() => {
                let i = rng.random_range(0..claims.len());
                let claim = claims[i];
                let cut = rng.random_range(claim.start + 1..=claim.end);
                claims[i] = ByteRange::new(claim.start, cut);
            }
            _ => {}
        }

        for (i, a) in claims.iter().enumerate() {
            for b in &claims[i + 1..] {
                assert!(
                    !overlaps(*a, *b),
                    "step {step}: claims {a:?} and {b:?} overlap"
                );
            }
            for c in completed.ranges() {
                assert!(
                    !overlaps(*a, *c),
                    "step {step}: claim {a:?} overlaps completed {c:?}"
                );
            }
        }
        let mut blocked = completed.clone();
        for claim in &claims {
            blocked.insert(*claim);
        }
        let claimed: i64 = claims.iter().map(|c| c.length()).sum();
        let remaining: i64 = blocked.gaps_within(TOTAL).iter().map(|g| g.length()).sum();
        assert_eq!(
            completed.total_bytes() + claimed + remaining,
            TOTAL,
            "step {step}: completed + claimed + remaining must cover the space exactly once"
        );
    }
}
