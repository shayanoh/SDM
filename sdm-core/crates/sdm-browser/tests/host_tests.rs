use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use sdm_browser::host::{HostEnvironment, handle};
use sdm_browser::status::read_status;
use sdm_resolve::SiteRegistry;

type Launches = Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>;

fn environment(dir: &Path) -> (HostEnvironment, Launches) {
    let launches: Launches = Arc::default();
    let recorded = Arc::clone(&launches);
    let env = HostEnvironment {
        data_dir: dir.to_path_buf(),
        app_executable: PathBuf::from("/Apps/SDM"),
        now: Box::new(|| SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)),
        launch: Box::new(move |program, args| {
            recorded
                .lock()
                .unwrap()
                .push((program.to_path_buf(), args.to_vec()));
            Ok(())
        }),
    };
    (env, launches)
}

fn reply(env: &HostEnvironment, request: &str) -> serde_json::Value {
    serde_json::from_slice(&handle(env, request.as_bytes()).unwrap()).unwrap()
}

#[test]
fn extension_status_is_saved_for_the_swiftui_compatible_reader() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    let got = reply(&env, r#"{"type":"extensionStatus","version":"1.1.0"}"#);
    assert_eq!(got["success"], true);
    let status = read_status(dir.path()).unwrap();
    assert_eq!(status.version, "1.1.0");
    // Seconds since 2001-01-01 (Swift's default `Date` encoding):
    // 1_800_000_000 - 978_307_200.
    assert_eq!(status.last_seen, 821_692_800.0);
}

#[test]
fn a_download_launches_the_app_with_add_url() {
    let dir = tempfile::tempdir().unwrap();
    let (env, launches) = environment(dir.path());
    let got = reply(
        &env,
        r#"{"type":"download","url":"https://example.com/a.zip","referrer":null}"#,
    );
    assert_eq!(got["success"], true);
    assert_eq!(
        *launches.lock().unwrap(),
        [(
            PathBuf::from("/Apps/SDM"),
            vec![
                "--add-url".to_string(),
                "https://example.com/a.zip".to_string()
            ]
        )]
    );
}

#[test]
fn only_http_and_https_downloads_are_handed_over() {
    let dir = tempfile::tempdir().unwrap();
    let (env, launches) = environment(dir.path());
    for url in ["file:///etc/passwd", "javascript:alert(1)", "not a url", ""] {
        let request = serde_json::json!({"type": "download", "url": url}).to_string();
        assert!(handle(&env, request.as_bytes()).is_err(), "{url}");
    }
    assert!(launches.lock().unwrap().is_empty());
}

#[test]
fn site_list_returns_the_registry_domains_and_their_version() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    let got = reply(&env, r#"{"type":"siteList"}"#);
    assert_eq!(got["success"], true);
    assert_eq!(got["version"], SiteRegistry::cookie_domains_version());
    let domains: Vec<String> = serde_json::from_value(got["domains"].clone()).unwrap();
    assert_eq!(domains, SiteRegistry::cookie_domains());
}

const SNAPSHOT: &str = r#"{"type":"cookies","siteListVersion":"old","cookies":[
  {"domain":".youtube.com","name":"SID","value":"a","path":"/","secure":true,"httpOnly":true},
  {"domain":"notyoutube.com","name":"X","value":"b","path":"/"}
]}"#;

#[test]
fn cookies_are_filtered_and_written_and_a_newer_snapshot_replaces_them() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    let got = reply(&env, SNAPSHOT);
    assert_eq!(got["success"], true);
    assert_eq!(got["written"], 1);
    assert_eq!(
        got["siteListVersion"],
        SiteRegistry::cookie_domains_version()
    );
    let file = dir.path().join("chrome-cookies.txt");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\ta"));
    assert!(!text.contains("notyoutube"));

    let second = r#"{"type":"cookies","siteListVersion":"x","cookies":[
      {"domain":".vimeo.com","name":"V","value":"c","path":"/"}]}"#;
    reply(&env, second);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\tV\tc") && !text.contains("SID"));
}

#[test]
fn bad_requests_fail_without_writing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let (env, launches) = environment(dir.path());
    for request in [
        "not json",
        "{}",
        r#"{"type":"nope"}"#,
        r#"{"type":"cookies","cookies":"x"}"#,
        r#"{"type":"extensionStatus"}"#,
    ] {
        assert!(handle(&env, request.as_bytes()).is_err(), "{request}");
    }
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    assert!(launches.lock().unwrap().is_empty());
}

fn framed(payload: &str) -> Vec<u8> {
    let mut wire = (payload.len() as u32).to_le_bytes().to_vec();
    wire.extend_from_slice(payload.as_bytes());
    wire
}

#[test]
fn run_answers_one_message_on_the_output_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    let mut input = framed(r#"{"type":"siteList"}"#);
    let mut output = Vec::new();
    assert_eq!(
        sdm_browser::host::run(&env, &mut input.as_slice(), &mut output),
        0
    );
    let mut reply = output.as_slice();
    let body = sdm_browser::framing::read_message(&mut reply).unwrap();
    assert!(reply.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["success"],
        true
    );
    input.clear();
}

#[test]
fn run_reports_a_failed_request_and_exits_nonzero_without_writing_files() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    let input = framed("not json");
    let mut output = Vec::new();
    assert_eq!(
        sdm_browser::host::run(&env, &mut input.as_slice(), &mut output),
        1
    );
    let body = sdm_browser::framing::read_message(&mut output.as_slice()).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["success"],
        false
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn run_exits_cleanly_on_a_truncated_or_empty_input() {
    let dir = tempfile::tempdir().unwrap();
    let (env, _) = environment(dir.path());
    for wire in [
        Vec::new(),
        vec![5, 0, 0, 0, b'a'],
        0u32.to_le_bytes().to_vec(),
    ] {
        let mut output = Vec::new();
        assert_eq!(
            sdm_browser::host::run(&env, &mut wire.as_slice(), &mut output),
            1
        );
        assert!(output.is_empty());
    }
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}
