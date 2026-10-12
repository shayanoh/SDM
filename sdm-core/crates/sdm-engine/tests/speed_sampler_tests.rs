use sdm_core::AppTiming;
use sdm_engine::SpeedSampler;

/// `SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)`.
fn sampler_1hz() -> SpeedSampler {
    SpeedSampler::new()
        .with_history_length(60)
        .with_averaging_window_seconds(2.0)
        .with_ticks_per_second(1)
}

#[test]
fn new_sampler_reports_zero() {
    assert_eq!(SpeedSampler::new().bytes_per_second(), 0.0);
}

#[test]
fn first_tick_reports_bytes_recorded_in_that_tick_scaled_to_one_second() {
    let mut sampler = sampler_1hz();
    sampler.record(1000);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1000.0);
}

#[test]
fn record_accumulates_within_one_tick() {
    let mut sampler = sampler_1hz();
    sampler.record(400);
    sampler.record(600);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1000.0);
}

#[test]
fn tick_scales_a_partial_second_window_up_to_bytes_per_second() {
    // At 5 ticks/second each tick covers 0.2 s, so 200 bytes in one tick is
    // 1000 bytes/s.
    let mut sampler = SpeedSampler::new()
        .with_history_length(60)
        .with_averaging_window_seconds(2.0)
        .with_ticks_per_second(5);
    sampler.record(200);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1000.0);
}

#[test]
fn bytes_per_second_averages_only_the_trailing_window() {
    // A 2-second window at 1 tick/second is 2 samples: the third tick's
    // 3000 must push the first tick's 1000 out of the average.
    let mut sampler = sampler_1hz();
    sampler.record(1000);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1000.0);
    sampler.record(1000);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1000.0);
    sampler.record(3000);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 2000.0);
}

#[test]
fn idle_reports_zero_immediately_rather_than_decaying() {
    let mut sampler = sampler_1hz();
    sampler.record(1_000_000);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 1_000_000.0);
    sampler.idle();
    assert_eq!(sampler.bytes_per_second(), 0.0);
}

#[test]
fn tick_after_idle_resumes_reporting_immediately() {
    let mut sampler = sampler_1hz();
    sampler.record(1000);
    sampler.tick();
    sampler.idle();
    assert_eq!(sampler.bytes_per_second(), 0.0);
    sampler.record(500);
    sampler.tick();
    assert_eq!(sampler.bytes_per_second(), 500.0);
}

#[test]
fn history_records_raw_per_second_estimates_in_order() {
    let mut sampler = sampler_1hz();
    sampler.record(100);
    sampler.tick();
    sampler.record(200);
    sampler.tick();
    assert_eq!(sampler.history(), [100.0, 200.0]);
}

#[test]
fn idle_appends_a_zero_sample_to_history() {
    let mut sampler = sampler_1hz();
    sampler.record(100);
    sampler.tick();
    sampler.idle();
    assert_eq!(sampler.history(), [100.0, 0.0]);
}

/// Regression: a heartbeat tick keeps calling `idle()` on every sampler for
/// every non-running item, every tick, indefinitely — the caller has no way
/// to know it already went idle and stop calling. Without this, `history`
/// (and therefore the whole `SpeedSampler` value) kept changing shape for a
/// full `historyLength` worth of ticks after any item went idle, purely from
/// harmless-looking repeated `idle()` calls, which defeated equality-based
/// change detection one layer up (`EngineController` comparing telemetry
/// snapshots to decide whether to republish).
#[test]
fn idle_called_again_while_already_idle_does_not_mutate_history_further() {
    let mut sampler = sampler_1hz();
    sampler.record(100);
    sampler.tick();
    sampler.idle();
    let after_first_idle = sampler.clone();
    sampler.idle();
    sampler.idle();
    sampler.idle();
    assert_eq!(sampler.history(), after_first_idle.history());
    assert_eq!(sampler.history(), [100.0, 0.0]);
}

#[test]
fn history_is_capped_at_its_length() {
    let mut sampler = SpeedSampler::new()
        .with_history_length(3)
        .with_averaging_window_seconds(2.0)
        .with_ticks_per_second(1);
    for value in 1..=5 {
        sampler.record(value * 100);
        sampler.tick();
    }
    assert_eq!(sampler.history(), [300.0, 400.0, 500.0]);
}

#[test]
fn default_history_length_scales_with_app_timing() {
    let mut sampler = SpeedSampler::new();
    let full = AppTiming::TICKS_PER_SECOND * 60 * 10;
    for value in 1..=(full + 5) {
        sampler.record(value as i64);
        sampler.tick();
    }
    assert_eq!(sampler.history().len(), full);
}

#[test]
fn recent_history_is_the_trailing_window_of_the_full_history() {
    let mut sampler = SpeedSampler::new()
        .with_history_length(10)
        .with_averaging_window_seconds(2.0)
        .with_ticks_per_second(1)
        .with_recent_length(3);
    for bytes in [100, 200, 300, 400, 500] {
        sampler.record(bytes);
        sampler.tick();
    }
    assert_eq!(sampler.history(), [100.0, 200.0, 300.0, 400.0, 500.0]);
    assert_eq!(sampler.recent_history(), [300.0, 400.0, 500.0]);
}

#[test]
fn recent_history_records_the_drop_to_idle_like_the_full_history() {
    let mut sampler = SpeedSampler::new()
        .with_history_length(10)
        .with_averaging_window_seconds(2.0)
        .with_ticks_per_second(1)
        .with_recent_length(3);
    sampler.record(100);
    sampler.tick();
    sampler.idle();
    sampler.idle();
    assert_eq!(sampler.recent_history(), [100.0, 0.0]);
    assert_eq!(sampler.recent_history(), sampler.history());
}
