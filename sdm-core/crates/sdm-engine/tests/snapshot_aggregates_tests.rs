use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
use sdm_engine::{
    EngineFigures, ItemSnapshot, LiveWindow, PackageAggregate, PackageSnapshot,
    average_recent_speed, estimated_seconds_remaining,
};
use uuid::Uuid;

struct Spec {
    total: Option<i64>,
    done: i64,
    state: ItemState,
    is_enabled: bool,
    bytes_per_second: f64,
    history: Vec<f64>,
}

fn spec() -> Spec {
    Spec {
        total: Some(1000),
        done: 0,
        state: ItemState::Stopped,
        is_enabled: true,
        bytes_per_second: 0.0,
        history: vec![],
    }
}

impl Spec {
    fn total(mut self, total: Option<i64>) -> Self {
        self.total = total;
        self
    }
    fn done(mut self, done: i64) -> Self {
        self.done = done;
        self
    }
    fn state(mut self, state: ItemState) -> Self {
        self.state = state;
        self
    }
    fn enabled(mut self, enabled: bool) -> Self {
        self.is_enabled = enabled;
        self
    }
    fn speed(mut self, speed: f64) -> Self {
        self.bytes_per_second = speed;
        self
    }
    fn history(mut self, history: &[f64]) -> Self {
        self.history = history.to_vec();
        self
    }
    fn build(self) -> ItemSnapshot {
        ItemSnapshot::new(
            Uuid::new_v4(),
            SourceUrl::new("https://example.com/file").unwrap(),
            "file",
            self.total,
            if self.done > 0 {
                RangeSet::from_ranges(&[ByteRange::new(0, self.done)])
            } else {
                RangeSet::new()
            },
            self.state,
            self.is_enabled,
            Some(true),
            0,
            4,
            self.bytes_per_second,
            self.history,
        )
    }
}

fn item() -> ItemSnapshot {
    spec().build()
}

fn package(items: Vec<ItemSnapshot>) -> PackageSnapshot {
    PackageSnapshot::new(Uuid::new_v4(), "p", Priority::Normal, items, 0.0, 0)
}

fn failed() -> ItemState {
    ItemState::Failed {
        reason: "x".to_string(),
    }
}

// MARK: - average_recent_speed / estimated_seconds_remaining

#[test]
fn average_recent_speed_falls_back_when_there_is_no_history() {
    assert_eq!(average_recent_speed(&[], 42.0), 42.0);
}

#[test]
fn average_recent_speed_uses_only_the_trailing_eta_window() {
    // Older samples than the window must not drag the average.
    let mut history = vec![9999.0; 10];
    history.extend(vec![100.0; LiveWindow::ETA_TICKS]);
    assert_eq!(average_recent_speed(&history, 0.0), 100.0);
}

#[test]
fn average_recent_speed_averages_a_short_history_over_its_own_length() {
    assert_eq!(average_recent_speed(&[100.0, 300.0], 0.0), 200.0);
}

#[test]
fn estimated_seconds_remaining_is_zero_without_speed_or_total() {
    assert_eq!(estimated_seconds_remaining(1000, 0, 0.0), 0.0);
    assert_eq!(estimated_seconds_remaining(0, 0, 100.0), 0.0);
}

#[test]
fn estimated_seconds_remaining_divides_the_remainder_by_speed() {
    assert_eq!(estimated_seconds_remaining(1000, 400, 100.0), 6.0);
}

// MARK: - PackageAggregate

#[test]
fn package_aggregate_of_no_items_is_empty() {
    assert_eq!(
        PackageAggregate::new(&[], 100.0),
        PackageAggregate::default()
    );
}

#[test]
fn package_aggregate_sums_bytes_and_speed_across_items() {
    let aggregate = PackageAggregate::new(
        &[
            spec()
                .total(Some(1000))
                .done(250)
                .state(ItemState::Running)
                .speed(10.0)
                .build(),
            spec()
                .total(Some(500))
                .done(500)
                .state(ItemState::Completed)
                .build(),
            spec()
                .total(None)
                .done(30)
                .state(ItemState::Running)
                .speed(5.0)
                .build(),
        ],
        0.0,
    );
    assert_eq!(aggregate.completed_bytes, 780);
    assert_eq!(aggregate.total_bytes, 1500);
    assert_eq!(aggregate.bytes_per_second, 15.0);
}

#[test]
fn package_aggregate_agrees_with_package_snapshots_own_aggregates() {
    let items = vec![
        spec()
            .total(Some(1000))
            .done(250)
            .state(ItemState::Running)
            .speed(10.0)
            .history(&[1.0, 2.0, 3.0])
            .build(),
        spec()
            .total(Some(700))
            .done(100)
            .state(ItemState::Queued)
            .history(&[5.0])
            .build(),
    ];
    let snapshot = package(items.clone());
    let aggregate = PackageAggregate::new(&items, 0.0);
    assert_eq!(aggregate.completed_bytes, snapshot.completed_bytes());
    assert_eq!(aggregate.total_bytes, snapshot.total_bytes());
    assert_eq!(aggregate.bytes_per_second, snapshot.bytes_per_second());
    assert_eq!(aggregate.speed_history, snapshot.bytes_per_second_history());
    // The aggregate the snapshot carries is the same computation.
    assert_eq!(snapshot.aggregate, aggregate);
}

#[test]
fn package_aggregate_history_aligns_shorter_histories_on_the_trailing_edge() {
    let aggregate = PackageAggregate::new(
        &[
            spec().history(&[1.0, 2.0, 3.0, 4.0]).build(),
            spec().history(&[10.0, 20.0]).build(),
            item(),
        ],
        0.0,
    );
    assert_eq!(aggregate.speed_history, [1.0, 2.0, 13.0, 24.0]);
}

#[test]
fn package_aggregate_history_keeps_only_the_sparkline_window() {
    let window = LiveWindow::PACKAGE_SPARKLINE_SAMPLES;
    let long: Vec<f64> = (0..window + 100).map(|v| v as f64).collect();
    let items = vec![
        spec().history(&long).build(),
        spec().history(&[1000.0]).build(),
    ];
    let aggregate = PackageAggregate::new(&items, 0.0);
    // Exactly the tail of what the uncapped sum would have been.
    let uncapped = package(items).bytes_per_second_history();
    assert_eq!(aggregate.speed_history, uncapped[uncapped.len() - window..]);
    assert_eq!(aggregate.speed_history.len(), window);
}

#[test]
fn package_aggregate_has_no_eta_when_nothing_is_running() {
    let aggregate = PackageAggregate::new(
        &[
            spec().total(Some(1000)).state(ItemState::Queued).build(),
            spec().total(Some(1000)).state(ItemState::Stopped).build(),
        ],
        100.0,
    );
    assert_eq!(aggregate.eta, 0.0);
}

#[test]
fn package_aggregate_eta_is_the_slowest_running_item_plus_the_queued_remainder() {
    let aggregate = PackageAggregate::new(
        &[
            // 600 left at 100 B/s -> 6 s
            spec()
                .total(Some(1000))
                .done(400)
                .state(ItemState::Running)
                .history(&[100.0])
                .build(),
            // 100 left at 50 B/s -> 2 s
            spec()
                .total(Some(200))
                .done(100)
                .state(ItemState::Running)
                .history(&[50.0])
                .build(),
            // 800 queued at the global 200 B/s -> 4 s
            spec()
                .total(Some(1000))
                .done(200)
                .state(ItemState::Queued)
                .build(),
            // Unknown size: contributes nothing rather than a negative.
            spec().total(None).done(50).state(ItemState::Queued).build(),
            spec().total(Some(5000)).state(ItemState::Stopped).build(),
        ],
        200.0,
    );
    assert_eq!(aggregate.eta, 10.0);
}

#[test]
fn package_aggregate_running_item_without_history_falls_back_to_its_current_speed() {
    let aggregate = PackageAggregate::new(
        &[spec()
            .total(Some(1000))
            .done(0)
            .state(ItemState::Running)
            .speed(250.0)
            .build()],
        0.0,
    );
    assert_eq!(aggregate.eta, 4.0);
}

// MARK: - EngineFigures

#[test]
fn engine_figures_of_no_packages_are_zero() {
    assert_eq!(EngineFigures::new(&[]), EngineFigures::default());
}

#[test]
fn engine_figures_count_running_and_queued() {
    let figures = EngineFigures::new(&[
        package(vec![
            spec().state(ItemState::Running).build(),
            spec().state(ItemState::Queued).build(),
            spec().state(ItemState::Queued).build(),
        ]),
        package(vec![
            spec().state(ItemState::Running).build(),
            spec().state(ItemState::Stopped).build(),
            spec().state(failed()).build(),
        ]),
    ]);
    assert_eq!(figures.running_count, 2);
    assert_eq!(figures.queued_count, 2);
}

#[test]
fn engine_figures_ring_averages_running_queued_and_completed_items_of_known_size() {
    let figures = EngineFigures::new(&[package(vec![
        spec()
            .total(Some(1000))
            .done(500)
            .state(ItemState::Running)
            .build(), // 0.5
        spec()
            .total(Some(200))
            .done(200)
            .state(ItemState::Completed)
            .build(), // 1.0
        spec()
            .total(Some(400))
            .done(0)
            .state(ItemState::Queued)
            .build(), // 0.0
        // Not ring states, or no known size: excluded entirely.
        spec()
            .total(Some(1000))
            .done(900)
            .state(ItemState::Stopped)
            .build(),
        spec().total(Some(1000)).done(900).state(failed()).build(),
        spec()
            .total(None)
            .done(10)
            .state(ItemState::Running)
            .build(),
        spec().total(Some(0)).state(ItemState::Running).build(),
    ])]);
    assert_eq!(figures.overall_fraction, 0.5);
    assert_eq!(figures.ring_completed_bytes, 700);
    assert_eq!(figures.ring_total_bytes, 1600);
}

#[test]
fn engine_figures_waiting_counts_enabled_unfinished_items_and_their_packages() {
    let figures = EngineFigures::new(&[
        package(vec![
            spec().state(ItemState::Stopped).build(),
            spec().state(ItemState::Completed).build(),
        ]),
        package(vec![
            spec().state(ItemState::Completed).build(),
            spec().state(ItemState::Queued).enabled(false).build(),
        ]),
        package(vec![
            spec().state(failed()).build(),
            spec().state(ItemState::Running).build(),
        ]),
    ]);
    assert_eq!(figures.waiting_item_count, 3);
    assert_eq!(figures.waiting_package_count, 2);
}
