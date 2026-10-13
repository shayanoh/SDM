use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sdm_resolve::BinaryLocator;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn present(paths: &[&str]) -> HashSet<PathBuf> {
    paths.iter().map(|s| p(s)).collect()
}

#[test]
fn locates_binary_in_first_search_path_that_has_it() {
    let present = present(&["/fake/b/yt-dlp"]);
    let locator = BinaryLocator::new()
        .with_search_paths(vec![p("/fake/a"), p("/fake/b")])
        .with_is_executable(move |path| present.contains(path));
    assert_eq!(locator.locate("yt-dlp"), Some(p("/fake/b/yt-dlp")));
}

#[test]
fn returns_nil_when_not_in_any_search_path() {
    let locator = BinaryLocator::new()
        .with_search_paths(vec![p("/fake/a")])
        .with_is_executable(|_| false);
    assert_eq!(locator.locate("ffmpeg"), None);
}

#[test]
fn override_wins_over_search_path_when_executable() {
    let present = present(&["/fake/a/yt-dlp", "/custom/yt-dlp"]);
    let locator = BinaryLocator::new()
        .with_search_paths(vec![p("/fake/a")])
        .with_is_executable(move |path| present.contains(path));
    locator.set_override(Some(p("/custom/yt-dlp")), "yt-dlp");
    assert_eq!(locator.locate("yt-dlp"), Some(p("/custom/yt-dlp")));
}

#[test]
fn non_executable_override_is_ignored() {
    let present = present(&["/fake/a/yt-dlp"]);
    let locator = BinaryLocator::new()
        .with_search_paths(vec![p("/fake/a")])
        .with_is_executable(move |path| present.contains(path));
    locator.set_override(Some(p("/custom/missing")), "yt-dlp");
    assert_eq!(locator.locate("yt-dlp"), Some(p("/fake/a/yt-dlp")));
}

#[test]
fn invalidate_forces_re_scan() {
    let present = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
    let shared = present.clone();
    let locator = BinaryLocator::new()
        .with_search_paths(vec![p("/fake/a")])
        .with_is_executable(move |path| shared.lock().unwrap().contains(path));
    assert_eq!(locator.locate("yt-dlp"), None);
    *present.lock().unwrap() = self::present(&["/fake/a/yt-dlp"]);
    assert_eq!(locator.locate("yt-dlp"), None); // memoized miss
    locator.invalidate();
    assert_eq!(locator.locate("yt-dlp"), Some(p("/fake/a/yt-dlp")));
}
