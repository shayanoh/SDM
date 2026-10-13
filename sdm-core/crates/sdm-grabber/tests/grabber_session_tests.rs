mod support;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sdm_grabber::{
    FakeProbeOrigin, FakeProbeOriginBehavior, GrabberBudget, GrabberSession, LinkProber, Stage,
    Verdict,
};
use support::{ManualClock, su, wait_until, within};

fn session_for(origin: &Arc<FakeProbeOrigin>) -> GrabberSession {
    GrabberSession::builder(LinkProber::new(origin.clone(), false)).build()
}

fn held() -> FakeProbeOriginBehavior {
    FakeProbeOriginBehavior {
        holds_until_released: true,
        ..FakeProbeOriginBehavior::default()
    }
}

fn headers(fields: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_extracts_and_probes_links() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        origin.set_behavior(
            FakeProbeOriginBehavior {
                status_code: 200,
                headers: headers(&[("content-type", "video/mp4"), ("content-length", "5000000")]),
                ..FakeProbeOriginBehavior::default()
            },
            &su("https://a.example.com/movie.mp4"),
        );

        let session = session_for(&origin);
        session
            .ingest_text("check out https://a.example.com/movie.mp4 now")
            .await;

        let snapshot = session.snapshot();
        assert_eq!(snapshot.total_count, 1);
        assert_eq!(snapshot.checked_count, 1);
        assert_eq!(
            snapshot.links.first().and_then(|l| l.verdict.clone()),
            Some(Verdict::Online)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_dedupes_a_repeated_url_across_calls() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url = su("https://a.example.com/movie.mp4");
        let session = session_for(&origin);

        session.ingest_urls(std::slice::from_ref(&url)).await;
        session.ingest_urls(std::slice::from_ref(&url)).await;

        assert_eq!(session.snapshot().total_count, 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_clusters_related_links_into_one_package() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let urls = [
            su("https://tv.example.com/season1/Show.S01E01.1080p.mkv"),
            su("https://tv.example.com/season1/Show.S01E02.1080p.mkv"),
        ];
        for url in &urls {
            origin.set_behavior(FakeProbeOriginBehavior::default(), url);
        }

        let session = session_for(&origin);
        session.ingest_urls(&urls).await;

        let snapshot = session.snapshot();
        assert_eq!(snapshot.packages.len(), 1);
        assert_eq!(snapshot.packages[0].link_ids.len(), 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probing_respects_the_global_concurrency_budget() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url_a = su("https://a.example.com/1.bin");
        let url_b = su("https://a.example.com/2.bin");
        origin.set_behavior(held(), &url_a);
        origin.set_behavior(held(), &url_b);

        let session = GrabberSession::builder(LinkProber::new(origin.clone(), false))
            .budget(GrabberBudget::new(1, 4, 4))
            .build();

        let ingest = {
            let session = session.clone();
            let urls = [url_a.clone(), url_b.clone()];
            tokio::spawn(async move { session.ingest_urls(&urls).await })
        };
        wait_until(|| !origin.request_log().is_empty()).await;
        let mid_flight = session.snapshot();
        assert_eq!(
            mid_flight
                .links
                .iter()
                .filter(|l| l.stage == Stage::Probing)
                .count(),
            1
        );
        assert_eq!(
            mid_flight
                .links
                .iter()
                .filter(|l| l.stage == Stage::Queued)
                .count(),
            1
        );

        origin.release(&url_a);
        origin.release(&url_b);
        ingest.await.unwrap();

        let last = session.snapshot();
        assert!(last.links.iter().all(|l| l.stage == Stage::Done));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probing_respects_the_per_host_budget_independently_of_the_global_budget() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let host_a_first = su("https://a.example.com/1.bin");
        let host_a_second = su("https://a.example.com/2.bin");
        let host_b = su("https://b.example.com/1.bin");
        for url in [&host_a_first, &host_a_second, &host_b] {
            origin.set_behavior(held(), url);
        }

        let session = GrabberSession::builder(LinkProber::new(origin.clone(), false))
            .budget(GrabberBudget::new(8, 1, 4))
            .build();

        let urls = [host_a_first.clone(), host_a_second.clone(), host_b.clone()];
        let ingest = {
            let session = session.clone();
            let urls = urls.clone();
            tokio::spawn(async move { session.ingest_urls(&urls).await })
        };
        wait_until(|| origin.request_log().len() >= 2).await;
        let mid_flight = session.snapshot();
        assert_eq!(
            mid_flight
                .links
                .iter()
                .filter(|l| l.stage == Stage::Probing)
                .count(),
            2
        );
        assert_eq!(
            mid_flight
                .links
                .iter()
                .filter(|l| l.stage == Stage::Queued)
                .count(),
            1
        );
        // The queued one is host A's second link: the per-host cap skipped
        // it, and the order in which the others launched is unchanged.
        let queued = mid_flight
            .links
            .iter()
            .find(|l| l.stage == Stage::Queued)
            .unwrap();
        assert_eq!(queued.original_url, host_a_second);

        for url in &urls {
            origin.release(url);
        }
        ingest.await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recheck_retries_the_given_links_regardless_of_verdict() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url_a = su("https://a.example.com/a.mp4");
        let url_b = su("https://a.example.com/b.mp4");
        let gone = FakeProbeOriginBehavior {
            status_code: 404,
            ..FakeProbeOriginBehavior::default()
        };
        origin.set_behavior(gone.clone(), &url_a);
        origin.set_behavior(gone, &url_b);

        let session = session_for(&origin);
        session.ingest_urls(&[url_a.clone(), url_b.clone()]).await;
        assert!(
            session
                .snapshot()
                .links
                .iter()
                .all(|l| l.verdict == Some(Verdict::Offline))
        );

        origin.set_behavior(
            FakeProbeOriginBehavior {
                status_code: 200,
                headers: headers(&[("content-length", "5000000")]),
                ..FakeProbeOriginBehavior::default()
            },
            &url_a,
        );

        let id_a = session
            .snapshot()
            .links
            .iter()
            .find(|l| l.original_url == url_a)
            .map(|l| l.id)
            .unwrap();
        session.recheck(&HashSet::from([id_a])).await;

        let links = session.snapshot().links;
        assert_eq!(
            links
                .iter()
                .find(|l| l.original_url == url_a)
                .and_then(|l| l.verdict.clone()),
            Some(Verdict::Online)
        );
        assert_eq!(
            links
                .iter()
                .find(|l| l.original_url == url_b)
                .and_then(|l| l.verdict.clone()),
            Some(Verdict::Offline)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_checks_stops_pending_probes_and_leaves_them_recheckable() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url_a = su("https://a.example.com/1.bin");
        let url_b = su("https://a.example.com/2.bin");
        origin.set_behavior(held(), &url_a);
        origin.set_behavior(held(), &url_b);

        let session = GrabberSession::builder(LinkProber::new(origin.clone(), false))
            .budget(GrabberBudget::new(1, 4, 4))
            .build();
        assert!(!session.snapshot().is_checking());

        let ingest = {
            let session = session.clone();
            let urls = [url_a.clone(), url_b.clone()];
            tokio::spawn(async move { session.ingest_urls(&urls).await })
        };
        wait_until(|| !origin.request_log().is_empty()).await;
        assert!(session.snapshot().is_checking());

        session.cancel_checks();
        origin.release(&url_a);
        origin.release(&url_b);
        ingest.await.unwrap();

        let snap = session.snapshot();
        assert!(!snap.is_checking());
        assert!(
            snap.links
                .iter()
                .any(|l| l.verdict == Some(Verdict::CheckFailed))
        );
        assert!(snap.recheckable_count() >= 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prune_older_than_removes_rows_past_the_cutoff_and_keeps_fresher_ones() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let clock = ManualClock::new();
        let session = GrabberSession::builder(LinkProber::new(origin.clone(), false))
            .clock(clock.provider())
            .build();

        session
            .ingest_urls(&[su("https://a.example/old.zip")])
            .await;
        clock.advance(Duration::from_secs(30));
        // Everything ingested from here on is strictly newer than `cutoff`.
        let cutoff = clock.now();
        clock.advance(Duration::from_secs(1));
        session
            .ingest_urls(&[su("https://b.example/new.zip")])
            .await;

        // A generous max age: nothing is old enough yet.
        session.prune_older_than(Duration::from_secs(3600), cutoff);
        assert_eq!(session.snapshot().links.len(), 2);

        // max age 0 with `now == cutoff`: rows added before `cutoff` go, the
        // one added after stays.
        session.prune_older_than(Duration::ZERO, cutoff);
        let urls: Vec<String> = session
            .snapshot()
            .links
            .iter()
            .map(|l| l.original_url.as_str().to_owned())
            .collect();
        assert_eq!(urls, vec!["https://b.example/new.zip".to_owned()]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prune_older_than_is_a_no_op_for_a_never_expired_list() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let clock = ManualClock::new();
        let session = GrabberSession::builder(LinkProber::new(origin.clone(), false))
            .clock(clock.provider())
            .build();
        session.ingest_urls(&[su("https://x.example/f.zip")]).await;
        session.prune_older_than(Duration::from_secs(3600), clock.now());
        assert_eq!(session.snapshot().links.len(), 1);
    })
    .await;
}

// MARK: - A dropped caller does not abort the work

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_an_ingest_caller_still_finishes_the_probes_and_reclusters() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url = su("https://a.example.com/movie.mp4");
        origin.set_behavior(held(), &url);
        let session = session_for(&origin);

        let ingest = session.ingest_urls(std::slice::from_ref(&url));
        // Poll it until the probe is parked at the origin, then drop it.
        let mut ingest = Box::pin(ingest);
        assert!(futures::poll!(ingest.as_mut()).is_pending());
        wait_until(|| !origin.request_log().is_empty()).await;
        assert_eq!(session.snapshot().links[0].stage, Stage::Probing);
        drop(ingest);

        origin.release(&url);
        // The recluster runs after the verdict is merged, so wait for it.
        wait_until(|| !session.snapshot().packages.is_empty()).await;

        let snapshot = session.snapshot();
        assert_eq!(snapshot.links[0].stage, Stage::Done);
        assert!(snapshot.links[0].verdict.is_some());
        assert_eq!(snapshot.checked_count, 1);
        assert_eq!(snapshot.packages.len(), 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_a_recheck_caller_still_finishes_the_probe() {
    within(async {
        let origin = Arc::new(FakeProbeOrigin::new());
        let url = su("https://a.example.com/movie.mp4");
        let session = session_for(&origin);
        session.ingest_urls(std::slice::from_ref(&url)).await;
        let id = session.snapshot().links[0].id;

        origin.set_behavior(held(), &url);
        let ids: HashSet<_> = [id].into_iter().collect();
        let before = origin.request_log().len();
        poll_once_then_drop_after(&session, &ids, || origin.request_log().len() > before).await;

        origin.release(&url);
        // The first ingest left a verdict behind, so wait on the stage: a
        // recheck puts the link back to `Probing` until its probe is merged.
        wait_until(|| session.snapshot().links[0].stage == Stage::Done).await;
        assert!(session.snapshot().links[0].verdict.is_some());
    })
    .await;
}

/// Starts `session.recheck(ids)`, waits until `parked` holds (the probe is
/// held at the origin), then drops the caller's future.
async fn poll_once_then_drop_after(
    session: &GrabberSession,
    ids: &HashSet<uuid::Uuid>,
    mut parked: impl FnMut() -> bool,
) {
    let mut recheck = Box::pin(session.recheck(ids));
    assert!(futures::poll!(recheck.as_mut()).is_pending());
    wait_until(&mut parked).await;
    drop(recheck);
}
