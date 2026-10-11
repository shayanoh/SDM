use sdm_core::AppTiming;

#[test]
fn ticks_per_second_is_positive() {
    assert!(std::hint::black_box(AppTiming::TICKS_PER_SECOND) > 0);
}

#[test]
fn ticks_per_second_defaults_to_five() {
    assert_eq!(AppTiming::TICKS_PER_SECOND, 5);
}
