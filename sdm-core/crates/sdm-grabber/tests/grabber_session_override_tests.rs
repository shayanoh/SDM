mod support;

use std::collections::HashSet;
use std::sync::Arc;

use sdm_grabber::{FakeProbeOrigin, GrabberSession, LinkProber};
use support::{su, within};

fn make_session() -> GrabberSession {
    GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false)).build()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setting_known_download_urls_marks_existing_links_as_duplicates() {
    within(async {
        let session = make_session();
        let url = su("https://a.example.com/movie.mp4");
        session.ingest_urls(std::slice::from_ref(&url)).await;
        assert!(!session.snapshot().links[0].is_duplicate);

        session.set_known_download_urls(HashSet::from([url]));
        assert!(session.snapshot().links[0].is_duplicate);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_marks_duplicate_immediately_when_already_known() {
    within(async {
        let session = make_session();
        let url = su("https://a.example.com/movie.mp4");
        session.set_known_download_urls(HashSet::from([url.clone()]));
        session.ingest_urls(&[url]).await;
        assert!(session.snapshot().links[0].is_duplicate);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_a_link_allows_it_to_be_reingested() {
    within(async {
        let session = make_session();
        let url = su("https://a.example.com/movie.mp4");
        session.ingest_urls(std::slice::from_ref(&url)).await;
        let first_id = session.snapshot().links[0].id;

        session.remove_link(first_id);
        assert_eq!(session.snapshot().total_count, 0);

        session.ingest_urls(&[url]).await;
        assert_eq!(session.snapshot().total_count, 1);
        assert_ne!(session.snapshot().links[0].id, first_id);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_a_package_removes_every_member_link_and_allows_reingesting() {
    within(async {
        let session = make_session();
        let url_a = su("https://example.com/Show.S01E01.mkv");
        let url_b = su("https://example.com/Show.S01E02.mkv");
        let url_c = su("https://other.example.com/unrelated.bin");
        session.ingest_urls(&[url_a.clone(), url_b, url_c]).await;
        assert_eq!(session.snapshot().total_count, 3);

        let packages = session.snapshot().packages;
        let show_package = packages
            .iter()
            .find(|p| p.link_ids.len() == 2)
            .expect("the two episodes cluster")
            .clone();

        session.remove_package(show_package.id);

        let snapshot = session.snapshot();
        assert_eq!(snapshot.total_count, 1);
        assert_eq!(snapshot.packages.len(), 1);
        assert!(!snapshot.packages.iter().any(|p| p.id == show_package.id));

        // Removed links are gone from the seen set too, so they can be
        // re-grabbed.
        session.ingest_urls(&[url_a]).await;
        assert_eq!(session.snapshot().total_count, 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clear_removes_every_link_and_package() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[
                su("https://example.com/a.zip"),
                su("https://other.example.com/b.zip"),
            ])
            .await;
        assert_eq!(session.snapshot().total_count, 2);

        session.clear();

        let snapshot = session.snapshot();
        assert_eq!(snapshot.total_count, 0);
        assert!(snapshot.packages.is_empty());

        // Cleared links can be re-grabbed, same as any removed link.
        session
            .ingest_urls(&[su("https://example.com/a.zip")])
            .await;
        assert_eq!(session.snapshot().total_count, 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn moving_a_link_overrides_automatic_clustering() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[
                su("https://a.example.com/alpha.bin"),
                su("https://b.example.com/beta.bin"),
            ])
            .await;

        let before = session.snapshot();
        assert_eq!(before.packages.len(), 2);
        let package_a_id = before.packages.first().unwrap().id;
        let id_b = before.packages.last().unwrap().link_ids[0];

        session.move_link(id_b, package_a_id);

        let after = session.snapshot();
        assert_eq!(after.packages.len(), 1);
        assert_eq!(after.packages[0].link_ids.len(), 2);
    })
    .await;
}

/// Added for the URL-identity review focus: a link is identified by the URL
/// string exactly as written, never by a normalised form of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_url_deduplicates_against_its_own_original_spelling() {
    within(async {
        let session = make_session();
        let spelled = "https://EXAMPLE.com:443/a%20b";

        session.ingest_urls(&[su(spelled)]).await;
        session.ingest_urls(&[su(spelled)]).await;
        let snapshot = session.snapshot();
        assert_eq!(snapshot.links.len(), 1);
        assert_eq!(snapshot.links[0].original_url.as_str(), spelled);
        assert!(!snapshot.links[0].is_duplicate);

        // The exact string marks it known; the string survives in the
        // snapshot.
        session.set_known_download_urls(HashSet::from([su(spelled)]));
        let snapshot = session.snapshot();
        assert!(snapshot.links[0].is_duplicate);
        assert_eq!(snapshot.links[0].original_url.as_str(), spelled);

        // A normalised spelling of the same address is a different identity:
        // it is neither "known" nor a repeat of the link already in the
        // session.
        session.set_known_download_urls(HashSet::from([su("https://example.com/a%20b")]));
        assert!(!session.snapshot().links[0].is_duplicate);
        session
            .ingest_urls(&[su("https://example.com/a%20b")])
            .await;
        assert_eq!(session.snapshot().links.len(), 2);
    })
    .await;
}
