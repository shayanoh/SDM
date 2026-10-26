use sdm_browser::cookies::{ChromeCookie, filter_cookies, render_netscape, write_cookie_file};

fn cookie(domain: &str, name: &str, value: &str) -> ChromeCookie {
    ChromeCookie {
        domain: domain.into(),
        name: name.into(),
        value: value.into(),
        path: "/".into(),
        secure: false,
        http_only: false,
        expiration_date: None,
    }
}

#[test]
fn a_chrome_cookie_parses_from_the_extensions_json() {
    let json = r#"[{"domain":".youtube.com","name":"SID","value":"abc","path":"/","secure":true,
        "httpOnly":true,"hostOnly":false,"expirationDate":1893456000.5,"session":false,"storeId":"0"}]"#;
    let parsed: Vec<ChromeCookie> = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].secure && parsed[0].http_only);
    assert_eq!(parsed[0].expiration_date, Some(1893456000.5));
}

#[test]
fn netscape_rows_follow_the_seven_field_layout() {
    let mut sid = cookie(".youtube.com", "SID", "abc");
    sid.secure = true;
    sid.expiration_date = Some(1893456000.9);
    let host_only = cookie("accounts.google.com", "G", "x y");
    let mut http_only = cookie(".google.com", "HSID", "h");
    http_only.http_only = true;
    let out = render_netscape(&[sid, host_only, http_only]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "# Netscape HTTP Cookie File");
    assert_eq!(
        lines[1],
        ".youtube.com\tTRUE\t/\tTRUE\t1893456000\tSID\tabc"
    );
    assert_eq!(lines[2], "accounts.google.com\tFALSE\t/\tFALSE\t0\tG\tx y");
    assert_eq!(
        lines[3],
        "#HttpOnly_.google.com\tTRUE\t/\tFALSE\t0\tHSID\th"
    );
    assert!(out.ends_with('\n'));
}

#[test]
fn filter_keeps_listed_sites_and_their_subdomains_only() {
    let kept = filter_cookies(vec![
        cookie(".youtube.com", "a", "1"),
        cookie("music.youtube.com", "b", "2"),
        cookie("YOUTUBE.COM", "c", "3"),
        cookie("notyoutube.com", "d", "4"),
        cookie("youtube.com.evil.net", "e", "5"),
        cookie("", "f", "6"),
    ]);
    let names: Vec<&str> = kept.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c"]);
}

#[test]
fn filter_drops_cookies_that_would_break_the_file() {
    let kept = filter_cookies(vec![
        cookie(".youtube.com", "tab", "a\tb"),
        cookie(".youtube.com", "newline", "a\nb"),
        cookie(".youtube.com", "carriage", "a\rb"),
        cookie(".youtube.com", "na\tme", "v"),
        cookie(".youtube.com", "ok", "fine; with=punctuation#"),
    ]);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "ok");
}

#[test]
fn many_cookies_keep_their_order() {
    let all: Vec<ChromeCookie> = (0..5000)
        .map(|i| cookie(".youtube.com", &format!("n{i}"), "v"))
        .collect();
    let out = render_netscape(&filter_cookies(all));
    assert_eq!(out.lines().count(), 5001);
    assert!(out.lines().nth(1).unwrap().contains("\tn0\t"));
    assert!(out.lines().last().unwrap().contains("\tn4999\t"));
}

#[test]
fn writing_replaces_the_whole_file_and_leaves_no_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chrome-cookies.txt");
    write_cookie_file(&path, "first\n").unwrap();
    write_cookie_file(&path, "second\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn writing_creates_the_missing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("SDM").join("chrome-cookies.txt");
    write_cookie_file(&path, "x\n").unwrap();
    assert!(path.exists());
}
