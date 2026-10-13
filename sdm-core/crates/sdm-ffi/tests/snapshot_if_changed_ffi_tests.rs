//! `snapshot_if_changed` hands the engine snapshot across the boundary only
//! when it is newer than the one the caller already holds.
mod support;

use support::{engine_over, package, payload, scratch_directory, within};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unchanged_engine_returns_nothing_and_a_change_returns_a_newer_snapshot() {
    within(async {
        let dir = scratch_directory();
        let (engine, _trash) = engine_over(payload(16), dir.path());
        engine.add(package("Alpha", "a.bin", false)).await.unwrap();

        // With no generation yet, the first call returns the snapshot.
        let first = engine
            .snapshot_if_changed(None)
            .await
            .unwrap()
            .expect("the first call has no generation to match");
        assert_eq!(first, engine.snapshot().await.unwrap());

        // Idle ticks change nothing: nothing crosses the boundary.
        for _ in 0..5 {
            engine.tick().await.unwrap();
            assert!(
                engine
                    .snapshot_if_changed(Some(first.generation))
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        // A visible change comes back with a larger generation.
        engine.add(package("Beta", "b.bin", false)).await.unwrap();
        let changed = engine
            .snapshot_if_changed(Some(first.generation))
            .await
            .unwrap()
            .expect("an added package is visible");
        assert!(changed.generation > first.generation);

        // And it is the new baseline.
        assert!(
            engine
                .snapshot_if_changed(Some(changed.generation))
                .await
                .unwrap()
                .is_none()
        );
        // A caller holding a stale generation is given the current snapshot.
        assert!(
            engine
                .snapshot_if_changed(Some(first.generation))
                .await
                .unwrap()
                .is_some()
        );
    })
    .await;
}
