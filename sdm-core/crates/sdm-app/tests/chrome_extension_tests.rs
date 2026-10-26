use std::time::{Duration, SystemTime};

use sdm_app::chrome::chrome_extension_dto;
use sdm_browser::cookies::write_cookie_file;
use sdm_browser::status::write_status;

fn now() -> SystemTime {
    SystemTime::now()
}

#[test]
fn a_fresh_install_is_not_connected_and_has_no_cookies() {
    let dir = tempfile::tempdir().unwrap();
    let dto = chrome_extension_dto(dir.path(), Some("1.1.0"), true, now());
    assert_eq!(dto.state, "notConnected");
    assert_eq!(dto.installed_version, None);
    assert_eq!(dto.bundled_version.as_deref(), Some("1.1.0"));
    assert_eq!(dto.cookies_synced_seconds_ago, None);
    assert!(dto.chrome_installed);
    assert!(dto.extension_folder.ends_with("ChromeExtension"));
}

#[test]
fn a_reporting_extension_with_synced_cookies_is_connected() {
    let dir = tempfile::tempdir().unwrap();
    let t = now();
    write_status(dir.path(), "1.1.0", t - Duration::from_secs(60)).unwrap();
    write_cookie_file(&dir.path().join("chrome-cookies.txt"), "x\n").unwrap();
    let dto = chrome_extension_dto(dir.path(), Some("1.1.0"), true, t + Duration::from_secs(3));
    assert_eq!(dto.state, "connected");
    assert_eq!(dto.installed_version.as_deref(), Some("1.1.0"));
    assert!(dto.cookies_synced_seconds_ago.unwrap() <= 5);
}

#[test]
fn an_older_reporting_version_is_outdated() {
    let dir = tempfile::tempdir().unwrap();
    let t = now();
    write_status(dir.path(), "1.0.1", t).unwrap();
    let dto = chrome_extension_dto(dir.path(), Some("1.1.0"), true, t);
    assert_eq!(dto.state, "outdated");
}
