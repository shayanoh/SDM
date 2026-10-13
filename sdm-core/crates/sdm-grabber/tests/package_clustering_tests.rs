use std::collections::HashSet;

use sdm_grabber::{ClusterableLink, PackageClustering};
use uuid::Uuid;

fn link(filename: &str) -> ClusterableLink {
    link_on(filename, "cdn.example.com")
}

fn link_on(filename: &str, host: &str) -> ClusterableLink {
    ClusterableLink::new(Uuid::new_v4(), filename, host, "/season1")
}

fn ids<'a>(links: impl IntoIterator<Item = &'a ClusterableLink>) -> HashSet<Uuid> {
    links.into_iter().map(|l| l.id).collect()
}

fn set(v: &[Uuid]) -> HashSet<Uuid> {
    v.iter().copied().collect()
}

#[test]
fn episodes_with_the_same_template_cluster_together() {
    let e1 = link("Show.S01E01.1080p.mkv");
    let e2 = link("Show.S01E02.1080p.mkv");
    let candidates = PackageClustering::cluster(&[e1.clone(), e2.clone()]);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name, "Show S01");
    assert_eq!(set(&candidates[0].link_ids), ids([&e1, &e2]));
    assert!(!candidates[0].is_archive);
}

#[test]
fn season_episode_pattern_keeps_the_full_season_number_not_just_its_first_digit() {
    // Regression case: a naive character-by-character common prefix over
    // "S01E01.1080p" / "S01E02.1080p" stops at "S0" — the very first digit
    // where the two diverge — which is meaningless. This must keep "S01"
    // whole and drop only the episode-specific remainder.
    let e1 = link("S01E01.1080p.mkv");
    let e2 = link("S01E02.1080p.mkv");
    let candidates = PackageClustering::cluster(&[e1, e2]);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name, "S01");
}

#[test]
fn season_episode_pattern_properly_splits_two_different_seasons() {
    let season1 = vec![link("Show.S01E01.1080p.mkv"), link("Show.S01E02.1080p.mkv")];
    let season2 = vec![link("Show.S02E01.1080p.mkv"), link("Show.S02E02.1080p.mkv")];

    let links: Vec<_> = season1.iter().chain(season2.iter()).cloned().collect();
    let candidates = PackageClustering::cluster(&links);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].name, "Show S01");
    assert_eq!(set(&candidates[0].link_ids), ids(&season1));
    assert_eq!(candidates[1].name, "Show S02");
    assert_eq!(set(&candidates[1].link_ids), ids(&season2));
}

#[test]
fn farsi_names_are_handled_correctly_with_random_names() {
    let links = vec![
        link("برنامه هفتگی.xlsx"),
        link("رزومه.docx"),
        link("سریال خوب.S01E01.کیفیت بالا.mkv"),
        link("سریال خوب.S01E02.کیفیت بالا.mkv"),
        link("سریال خوب.S01E03.کیفیت بالا.mkv"),
        link("سریال خوب.S02E01.کیفیت بالا.mkv"),
        link("سریال خوب.txt"),
        link("کتاب اول.pdf"),
        link("کتاب دوم.pdf"),
    ];

    let candidates = PackageClustering::cluster(&links);
    assert_eq!(candidates.len(), 7);
    let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
    for expected in [
        "سریال خوب S01",
        "سریال خوب S02",
        "کتاب اول",
        "کتاب دوم",
        "برنامه هفتگی",
        "رزومه",
        "سریال خوب",
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
    let all: HashSet<Uuid> = candidates
        .iter()
        .flat_map(|c| c.link_ids.iter().copied())
        .collect();
    assert_eq!(all, ids(&links));
}

#[test]
fn dots_and_dashes_in_a_package_name_become_spaces() {
    let a = link("the.matrix.1999.bluray.mkv");
    let b = link("the.matrix.1999.bluray.nfo");
    let candidates = PackageClustering::cluster(&[a, b]);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name, "The Matrix 1999 Bluray");
}

#[test]
fn archive_parts_lock_together_regardless_of_template() {
    let parts = vec![
        link("Movie.part01.rar"),
        link("Movie.part02.rar"),
        link("Movie.part03.rar"),
    ];
    let candidates = PackageClustering::cluster(&parts);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name, "Movie");
    assert!(candidates[0].is_archive);
    assert_eq!(set(&candidates[0].link_ids), ids(&parts));
}

#[test]
fn singleton_templates_group_by_name() {
    let a = link("readme.txt");
    let b = link("changelog.md");
    let candidates = PackageClustering::cluster(&[a.clone(), b.clone()]);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].name, "Readme");
    assert_eq!(set(&candidates[0].link_ids), ids([&a]));
    assert_eq!(candidates[1].name, "Changelog");
    assert_eq!(set(&candidates[1].link_ids), ids([&b]));
}

#[test]
fn dissimilar_templates_on_different_hosts_stay_separate() {
    let a = link_on("readme.txt", "one.example.com");
    let b = link_on("changelog.md", "two.example.com");
    let candidates = PackageClustering::cluster(&[a.clone(), b.clone()]);

    assert_eq!(candidates.len(), 2);
    let all: HashSet<Uuid> = candidates
        .iter()
        .flat_map(|c| c.link_ids.iter().copied())
        .collect();
    assert_eq!(all, ids([&a, &b]));
}

#[test]
fn clustering_empty_input_returns_no_packages() {
    assert!(PackageClustering::cluster(&[]).is_empty());
}

#[test]
fn output_order_is_deterministic_across_repeated_calls() {
    let links: Vec<_> = (0..12)
        .map(|i| link_on(&format!("file{i}.bin"), &format!("h{i}.example.com")))
        .collect();
    let first: Vec<String> = PackageClustering::cluster(&links)
        .into_iter()
        .map(|c| c.name)
        .collect();
    let second: Vec<String> = PackageClustering::cluster(&links)
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(first, second);
}
