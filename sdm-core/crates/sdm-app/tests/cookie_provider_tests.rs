use std::sync::{Arc, RwLock};

use sdm_app::protocol::SettingsDto;
use sdm_app::settings::cookie_provider;

fn settings(source: &str) -> Arc<RwLock<SettingsDto>> {
    Arc::new(RwLock::new(SettingsDto {
        cookie_source: source.into(),
        ..SettingsDto::default()
    }))
}

fn write_cookie_file(dir: &std::path::Path) {
    std::fs::write(
        dir.join("chrome-cookies.txt"),
        "# Netscape HTTP Cookie File\n",
    )
    .unwrap();
}

#[test]
fn chrome_with_the_extension_file_passes_a_private_copy() {
    let dir = tempfile::tempdir().unwrap();
    write_cookie_file(dir.path());
    let provider = cookie_provider(&settings("chrome"), dir.path().to_path_buf());
    let lease = provider();
    assert_eq!(lease.arguments()[0], "--cookies");
    assert!(std::path::Path::new(&lease.arguments()[1]).exists());
}

#[test]
fn chrome_without_the_file_runs_without_cookies_and_says_why_on_a_sign_in_failure() {
    let dir = tempfile::tempdir().unwrap();
    let provider = cookie_provider(&settings("chrome"), dir.path().to_path_buf());
    let lease = provider();
    assert!(lease.arguments().is_empty());
    assert!(lease.is_chrome_unavailable());
}

#[test]
fn other_browsers_keep_cookies_from_browser() {
    let dir = tempfile::tempdir().unwrap();
    write_cookie_file(dir.path());
    let provider = cookie_provider(&settings("firefox"), dir.path().to_path_buf());
    assert_eq!(
        provider().arguments(),
        ["--cookies-from-browser", "firefox"]
    );
    let provider = cookie_provider(&settings("none"), dir.path().to_path_buf());
    assert!(provider().arguments().is_empty());
}

#[test]
fn the_choice_is_read_fresh_on_every_run() {
    let dir = tempfile::tempdir().unwrap();
    write_cookie_file(dir.path());
    let shared = settings("none");
    let provider = cookie_provider(&shared, dir.path().to_path_buf());
    assert!(provider().arguments().is_empty());
    shared.write().unwrap().cookie_source = "chrome".into();
    assert_eq!(provider().arguments()[0], "--cookies");
}
