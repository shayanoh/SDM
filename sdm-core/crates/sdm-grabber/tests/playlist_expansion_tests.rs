mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use sdm_core::{
    AudioCodec, LinkResolver, MediaContainer, RefreshedFormat, ResolveError, ResolvedMedia,
    ResolvedTarget, SourceUrl, VideoCodec,
};
use sdm_grabber::{FakeProbeOrigin, GrabberBudget, GrabberSession, LinkProber, MediaRowState};
use support::{FakeLinkResolver, af, single_media, su, vf, wait_until, within};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn entry(index: usize, title: String) -> ResolvedMedia {
    ResolvedMedia::new("youtube", format!("vid{index}"), title, None, vec![])
        .with_source_url(su(&format!("https://www.youtube.com/watch?v=vid{index}")))
}

fn pair() -> Vec<sdm_core::MediaFormat> {
    vec![
        vf("137", 720, VideoCodec::H264, MediaContainer::Mp4),
        af("140", AudioCodec::Aac, MediaContainer::M4a),
    ]
}

fn playlist_resolver(title: &'static str, count: usize, total_available: i64) -> FakeLinkResolver {
    let entries: Vec<ResolvedMedia> = (1..=count)
        .map(|i| entry(i, format!("Episode {i}")))
        .collect();
    FakeLinkResolver::new(move |url| {
        let parsed = url.as_url().expect("test URLs parse");
        if url.as_str().contains("list=") || parsed.path().starts_with("/playlist") {
            return Ok(ResolvedTarget::Playlist {
                title: title.to_owned(),
                entries: entries.clone(),
                total_available,
            });
        }
        let id = parsed
            .query_pairs()
            .find(|(name, _)| name == "v")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_else(|| "?".into());
        Ok(single_media(&id, "Episode", pair()))
    })
}

fn session_with(resolver: impl LinkResolver + 'static) -> GrabberSession {
    GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
        .resolver(Arc::new(resolver))
        .build()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_playlist_url_expands_into_one_row_per_entry_in_one_package() {
    within(async {
        let s = session_with(playlist_resolver("My Series", 5, 12));
        s.ingest_urls(&[su("https://www.youtube.com/playlist?list=PL123")])
            .await;
        let snap = s.snapshot();
        assert_eq!(snap.media_rows.len(), 5);
        let groups: std::collections::HashSet<_> = snap
            .media_rows
            .iter()
            .filter_map(|r| r.playlist_group)
            .collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(snap.packages.len(), 1);
        assert_eq!(snap.packages[0].name, "My Series");
        assert_eq!(snap.packages[0].note.as_deref(), Some("5 of 12 videos"));
        assert_eq!(snap.packages[0].link_ids.len(), 5);
        assert!(
            snap.media_rows
                .iter()
                .all(|r| r.state == MediaRowState::Resolved)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn playlist_entry_rows_use_their_own_source_url() {
    within(async {
        let entries = vec![
            ResolvedMedia::new("", "t1", "Track 1", None, vec![])
                .with_source_url(su("https://soundcloud.com/artist/track-1")),
            ResolvedMedia::new("", "t2", "Track 2", None, vec![])
                .with_source_url(su("https://soundcloud.com/artist/track-2")),
        ];
        let resolver = FakeLinkResolver::new(move |url| {
            let parsed = url.as_url().expect("test URLs parse");
            if parsed.path().contains("/sets/") {
                return Ok(ResolvedTarget::Playlist {
                    title: "Set".into(),
                    entries: entries.clone(),
                    total_available: 2,
                });
            }
            let last = parsed.path().rsplit('/').next().unwrap_or("").to_owned();
            Ok(single_media(&last, &last, pair()))
        })
        .with_handled_hosts(&["soundcloud.com"]);
        let s = session_with(resolver);
        s.ingest_urls(&[su("https://soundcloud.com/artist/sets/mix")])
            .await;
        let rows = s.snapshot().media_rows;
        assert_eq!(rows.len(), 2);
        let urls: std::collections::HashSet<SourceUrl> =
            rows.iter().map(|r| r.source_url.clone()).collect();
        assert_eq!(
            urls,
            std::collections::HashSet::from([
                su("https://soundcloud.com/artist/track-1"),
                su("https://soundcloud.com/artist/track-2"),
            ])
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fully_listed_playlist_has_no_truncation_note() {
    within(async {
        let s = session_with(playlist_resolver("Short", 3, 3));
        s.ingest_urls(&[su("https://www.youtube.com/playlist?list=X")])
            .await;
        assert_eq!(s.snapshot().packages[0].note, None);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_a_playlist_package_clears_its_note() {
    within(async {
        let s = session_with(playlist_resolver("Gone", 4, 9));
        s.ingest_urls(&[su("https://www.youtube.com/playlist?list=Y")])
            .await;
        let package_id = s.snapshot().packages[0].id;
        s.remove_package(package_id);
        assert!(s.snapshot().packages.is_empty());
        assert!(s.snapshot().media_rows.is_empty());
    })
    .await;
}

/// A resolver whose per-entry `resolve` parks at a gate until released,
/// counting how many calls have started and the peak in flight. It stands in
/// for the Swift `ConcurrencyTrackingResolver` (which slept 30 ms per
/// resolve) and `GatedPlaylistResolver`.
struct GatedPlaylistResolver {
    entries: Vec<ResolvedMedia>,
    started: AtomicUsize,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
    gate: watch::Sender<bool>,
}

impl GatedPlaylistResolver {
    fn new(count: usize) -> Arc<Self> {
        Arc::new(Self {
            entries: (1..=count).map(|i| entry(i, format!("E{i}"))).collect(),
            started: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            gate: watch::channel(false).0,
        })
    }

    fn started_count(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    fn release_all(&self) {
        self.gate.send_replace(true);
    }
}

#[async_trait]
impl LinkResolver for GatedPlaylistResolver {
    fn can_handle(&self, url: &SourceUrl) -> bool {
        url.as_url()
            .and_then(|parsed| parsed.host_str())
            .is_some_and(|host| host.contains("youtube"))
    }

    async fn resolve(&self, url: &SourceUrl) -> Result<ResolvedTarget, ResolveError> {
        if url.as_str().contains("list=") {
            return Ok(ResolvedTarget::Playlist {
                title: "P".into(),
                entries: self.entries.clone(),
                total_available: self.entries.len() as i64,
            });
        }
        self.started.fetch_add(1, Ordering::SeqCst);
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        let mut gate = self.gate.subscribe();
        let _ = gate.wait_for(|released| *released).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(single_media("v", "E", pair()))
    }

    async fn refresh(
        &self,
        _source_url: &SourceUrl,
        format_id: &str,
        _cancel: &CancellationToken,
    ) -> Result<RefreshedFormat, ResolveError> {
        Ok(RefreshedFormat::new(su("https://gv/x"), None, format_id))
    }
}

/// Lets the test keep a handle on the resolver the session owns.
struct Shared(Arc<GatedPlaylistResolver>);

#[async_trait]
impl LinkResolver for Shared {
    fn can_handle(&self, url: &SourceUrl) -> bool {
        self.0.can_handle(url)
    }

    async fn resolve(&self, url: &SourceUrl) -> Result<ResolvedTarget, ResolveError> {
        self.0.resolve(url).await
    }

    async fn refresh(
        &self,
        source_url: &SourceUrl,
        format_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RefreshedFormat, ResolveError> {
        self.0.refresh(source_url, format_id, cancel).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn playlist_entry_resolves_are_capped_at_max_concurrent_resolves() {
    within(async {
        let resolver = GatedPlaylistResolver::new(20);
        let session =
            GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
                .budget(GrabberBudget::new(32, 4, 3))
                .resolver(Arc::new(Shared(resolver.clone())))
                .build();
        let ingest = {
            let session = session.clone();
            tokio::spawn(async move {
                session
                    .ingest_urls(&[su("https://www.youtube.com/playlist?list=PLx")])
                    .await
            })
        };

        // The cap is reached, then given every chance to be exceeded.
        wait_until(|| resolver.started_count() >= 3).await;
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        assert_eq!(resolver.started_count(), 3);

        resolver.release_all();
        ingest.await.unwrap();
        assert_eq!(session.snapshot().media_rows.len(), 20);
        let peak = resolver.peak.load(Ordering::SeqCst);
        assert!(peak <= 3);
        assert!(peak >= 2); // it did actually parallelize some
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_playlist_can_be_added_again_after_its_entries_were_handed_off() {
    within(async {
        let session = session_with(playlist_resolver("Repeat", 4, 4));
        let url = su("https://www.youtube.com/playlist?list=PLrepeat");

        session.ingest_urls(std::slice::from_ref(&url)).await;
        assert_eq!(session.snapshot().media_rows.len(), 4);

        // Simulate the handoff removing every entry row.
        for row in session.snapshot().media_rows {
            session.remove_link(row.id);
        }
        assert!(session.snapshot().media_rows.is_empty());

        // The same playlist URL must ingest again - the origin URL was left
        // in the seen set before this fix and silently deduped.
        session.ingest_urls(&[url]).await;
        assert_eq!(session.snapshot().media_rows.len(), 4);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_checks_stops_playlist_entries_still_resolving() {
    within(async {
        let resolver = GatedPlaylistResolver::new(12);
        let session =
            GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false))
                .budget(GrabberBudget::new(16, 4, 2))
                .resolver(Arc::new(Shared(resolver.clone())))
                .build();

        let ingest = {
            let session = session.clone();
            tokio::spawn(async move {
                session
                    .ingest_urls(&[su("https://www.youtube.com/playlist?list=PLx")])
                    .await
            })
        };
        wait_until(|| resolver.started_count() > 0).await;

        session.cancel_checks();
        resolver.release_all();
        ingest.await.unwrap();

        let snap = session.snapshot();
        assert_eq!(snap.media_rows.len(), 12);
        assert!(
            snap.media_rows
                .iter()
                .any(|r| r.state == MediaRowState::Failed("Check cancelled".into()))
        );
        assert!(!snap.is_checking());
        assert!(snap.recheckable_count() >= 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recheck_re_resolves_failed_media_rows() {
    within(async {
        let counter = Arc::new(AtomicUsize::new(0));
        let resolver = {
            let counter = counter.clone();
            FakeLinkResolver::new(move |_| {
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(ResolveError::YtDlpFailed {
                        stderr_tail: "temporary blip".into(),
                    });
                }
                Ok(single_media("v", "Now works", pair()))
            })
        };
        let session = session_with(resolver);
        session
            .ingest_urls(&[su("https://www.youtube.com/watch?v=abc")])
            .await;
        assert!(matches!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Failed(_)
        ));
        assert_eq!(session.snapshot().recheckable_count(), 1);

        session.recheck_failed().await;
        assert_eq!(
            session.snapshot().media_rows[0].state,
            MediaRowState::Resolved
        );
        assert_eq!(session.snapshot().recheckable_count(), 0);
    })
    .await;
}
