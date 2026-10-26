use sdm_browser::cookies::write_cookie_file;
use sdm_browser::lease::{
    CookieFileState, cookie_file_state, lease_for_chrome, sweep_stale_leases,
};

fn data_dir_with_cookies(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_cookie_file(&dir.path().join("chrome-cookies.txt"), text).unwrap();
    dir
}

#[test]
fn a_missing_file_is_missing_and_gives_an_unavailable_lease() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        cookie_file_state(dir.path()),
        CookieFileState::Missing
    ));
    let lease = lease_for_chrome(dir.path());
    assert!(lease.is_chrome_unavailable());
    assert!(lease.arguments().is_empty());
}

#[test]
fn a_present_file_reports_when_it_was_written() {
    let dir = data_dir_with_cookies("x\n");
    let CookieFileState::Present { modified } = cookie_file_state(dir.path()) else {
        panic!("expected a present file");
    };
    assert!(modified.elapsed().unwrap().as_secs() < 60);
}

#[test]
fn a_lease_points_yt_dlp_at_a_private_copy_that_goes_away_with_it() {
    let dir = data_dir_with_cookies("# Netscape HTTP Cookie File\nrow\n");
    let lease = lease_for_chrome(dir.path());
    assert!(!lease.is_chrome_unavailable());
    let arguments = lease.arguments().to_vec();
    assert_eq!(arguments[0], "--cookies");
    let copy = std::path::PathBuf::from(&arguments[1]);
    assert_ne!(copy, dir.path().join("chrome-cookies.txt"));
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "# Netscape HTTP Cookie File\nrow\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // yt-dlp rewrites its copy; the source stays as the host wrote it.
    std::fs::write(&copy, "rewritten").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("chrome-cookies.txt")).unwrap(),
        "# Netscape HTTP Cookie File\nrow\n"
    );
    drop(lease);
    assert!(!copy.exists());
}

#[test]
fn two_leases_get_two_different_copies() {
    let dir = data_dir_with_cookies("x\n");
    let a = lease_for_chrome(dir.path());
    let b = lease_for_chrome(dir.path());
    assert_ne!(a.arguments()[1], b.arguments()[1]);
}

#[test]
fn the_startup_sweep_removes_copies_a_crash_left_behind() {
    let dir = data_dir_with_cookies("x\n");
    let leases = dir.path().join("cookie-leases");
    std::fs::create_dir_all(&leases).unwrap();
    std::fs::write(leases.join("old.txt"), "stale").unwrap();
    sweep_stale_leases(dir.path());
    assert!(std::fs::read_dir(&leases).map_or(true, |mut d| d.next().is_none()));
    assert!(dir.path().join("chrome-cookies.txt").exists());
    sweep_stale_leases(&dir.path().join("nonexistent"));
}
