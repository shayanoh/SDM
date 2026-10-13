mod support;

use std::sync::Arc;

use sdm_grabber::{FakeProbeOrigin, GrabberSession, LinkProber};
use support::{su, within};

fn make_session() -> GrabberSession {
    GrabberSession::builder(LinkProber::new(Arc::new(FakeProbeOrigin::new()), false)).build()
}

fn names(session: &GrabberSession) -> Vec<String> {
    session
        .snapshot()
        .packages
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rename_package_moves_every_member_link_under_the_new_name() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[su("https://example.com/a.zip")])
            .await;
        let original_id = session.snapshot().packages[0].id;

        session.rename_package(original_id, "My Archive");

        let snapshot = session.snapshot();
        assert_eq!(names(&session), vec!["My Archive".to_owned()]);
        assert_eq!(snapshot.packages[0].link_ids.len(), 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn merge_packages_combines_both_into_the_destination_name() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[
                su("https://example.com/a.zip"),
                su("https://other.example.com/b.zip"),
            ])
            .await;
        let packages = session.snapshot().packages;
        if packages.len() < 2 {
            // Clustering may already have grouped these; force two packages
            // via a manual rename so the merge itself is what's under test.
            session.rename_package(packages[0].id, "First");
            for package in session.snapshot().packages {
                if package.name != "First" {
                    session.rename_package(package.id, "Second");
                }
            }
            let before_merge = session.snapshot().packages;
            let source = before_merge.iter().find(|p| p.name == "Second").unwrap();
            let destination = before_merge.iter().find(|p| p.name == "First").unwrap();
            session.merge_packages(source.id, destination.id);
            let merged = session.snapshot();
            assert_eq!(names(&session), vec!["First".to_owned()]);
            assert_eq!(merged.packages[0].link_ids.len(), 2);
            return;
        }
        session.merge_packages(packages[1].id, packages[0].id);
        let merged = session.snapshot();
        assert_eq!(names(&session), vec![packages[0].name.clone()]);
        assert_eq!(merged.packages[0].link_ids.len(), 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn split_package_gives_each_link_its_own_package() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[
                su("https://example.com/Show.S01E01.mkv"),
                su("https://example.com/Show.S01E02.mkv"),
            ])
            .await;
        let original_id = session.snapshot().packages[0].id;
        assert_eq!(session.snapshot().packages[0].link_ids.len(), 2);

        session.split_package(original_id);

        let snapshot = session.snapshot();
        assert_eq!(snapshot.packages.len(), 2);
        assert!(snapshot.packages.iter().all(|p| p.link_ids.len() == 1));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn renaming_a_package_to_an_existing_name_merges_them_instead() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[
                su("https://example.com/a.zip"),
                su("https://other.example.com/b.zip"),
            ])
            .await;
        let mut packages = session.snapshot().packages;
        if packages.len() < 2 {
            session.rename_package(packages[0].id, "First");
            for package in session.snapshot().packages {
                if package.name != "First" {
                    session.rename_package(package.id, "Second");
                }
            }
            packages = session.snapshot().packages;
        }
        assert_eq!(packages.len(), 2);

        // Renaming the second package to the first's exact current name must
        // merge them into one - a duplicate name is not a valid end state.
        let target_name = packages[0].name.clone();
        session.rename_package(packages[1].id, &target_name);

        let merged = session.snapshot();
        assert_eq!(names(&session), vec![target_name]);
        assert_eq!(merged.packages[0].link_ids.len(), 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_packages_identity_survives_reclustering_under_an_unchanged_name() {
    within(async {
        let session = make_session();
        session
            .ingest_urls(&[su("https://example.com/a.zip")])
            .await;
        let original = session.snapshot().packages[0].clone();

        // Ingesting more links triggers another recluster pass; the existing
        // package's own id must not change just because clustering ran again
        // - otherwise any id-keyed UI state (a collapsed/expanded
        // disclosure, an in-flight context menu) would reset every time a
        // new, unrelated link came in.
        session
            .ingest_urls(&[su("https://other.example.com/c.zip")])
            .await;

        let after = session
            .snapshot()
            .packages
            .into_iter()
            .find(|p| p.name == original.name);
        assert_eq!(after.map(|p| p.id), Some(original.id));
    })
    .await;
}
