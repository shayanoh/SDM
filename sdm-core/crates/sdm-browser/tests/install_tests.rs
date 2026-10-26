use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sdm_browser::install::{
    EXTENSION_ID, ExtensionState, HOST_NAME, Platform, bundled_version, extension_state,
    manifest_is_usable, manifest_json, manifest_path, prepare_extension_folder, write_manifest,
};
use sdm_browser::status::ExtensionStatus;

#[test]
fn the_manifest_names_the_host_the_binary_and_only_our_extension() {
    let json = manifest_json(Path::new("/Apps/SDM.app/Contents/MacOS/sdm-tauri"));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["name"], HOST_NAME);
    assert_eq!(value["name"], "com.shayanoh.sdm.chrome_extension");
    assert_eq!(value["type"], "stdio");
    assert_eq!(value["path"], "/Apps/SDM.app/Contents/MacOS/sdm-tauri");
    assert_eq!(
        value["allowed_origins"],
        serde_json::json!([format!("chrome-extension://{EXTENSION_ID}/")])
    );
    assert_eq!(EXTENSION_ID, "fnnpcbmjpapeohfmpiefbocloodhlgjf");
}

#[test]
fn the_manifest_lives_where_chrome_looks_for_it() {
    let support = Path::new("/Users/u/Library/Application Support");
    let config = Path::new("/home/u/.config");
    let data = Path::new("/data/SDM");
    assert_eq!(
        manifest_path(Platform::MacOs, support, data),
        PathBuf::from(
            "/Users/u/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.shayanoh.sdm.chrome_extension.json"
        )
    );
    assert_eq!(
        manifest_path(Platform::Linux, config, data),
        PathBuf::from(
            "/home/u/.config/google-chrome/NativeMessagingHosts/com.shayanoh.sdm.chrome_extension.json"
        )
    );
    // Windows keeps the file in SDM's folder; the registry points at it.
    assert_eq!(
        manifest_path(Platform::Windows, config, data),
        PathBuf::from("/data/SDM/com.shayanoh.sdm.chrome_extension.json")
    );
}

#[test]
fn a_manifest_is_usable_when_it_exists_and_its_binary_does() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("sdm");
    let manifest = dir.path().join("host.json");
    assert!(!manifest_is_usable(&manifest));
    write_manifest(&manifest, &binary).unwrap();
    assert!(!manifest_is_usable(&manifest), "the binary is missing");
    std::fs::write(&binary, "x").unwrap();
    assert!(manifest_is_usable(&manifest));
    std::fs::write(&manifest, "not json").unwrap();
    assert!(!manifest_is_usable(&manifest));
}

fn write_extension(dir: &Path, version: &str, background: &str) {
    std::fs::create_dir_all(dir.join("images")).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(r#"{{"version":"{version}"}}"#),
    )
    .unwrap();
    std::fs::write(dir.join("background.js"), background).unwrap();
    std::fs::write(dir.join("images/icon.png"), [1u8, 2, 3]).unwrap();
}

#[test]
fn preparing_copies_the_extension_to_a_stable_folder_and_replaces_a_changed_one() {
    let root = tempfile::tempdir().unwrap();
    let bundled = root.path().join("bundle/chrome-extension");
    let data = root.path().join("data");
    write_extension(&bundled, "1.1.0", "v1");
    let folder = prepare_extension_folder(&bundled, &data).unwrap();
    assert_eq!(folder, data.join("ChromeExtension"));
    assert_eq!(bundled_version(&folder).as_deref(), Some("1.1.0"));
    assert!(folder.join("images/icon.png").exists());

    // Unchanged: the folder is left alone (Chrome may have it open).
    let marker = folder.join("background.js");
    let before = std::fs::metadata(&marker).unwrap().modified().unwrap();
    prepare_extension_folder(&bundled, &data).unwrap();
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        before
    );

    // Changed content and a file removed from the bundle: the copy follows.
    write_extension(&bundled, "1.2.0", "v2");
    std::fs::remove_file(bundled.join("images/icon.png")).unwrap();
    prepare_extension_folder(&bundled, &data).unwrap();
    assert_eq!(bundled_version(&folder).as_deref(), Some("1.2.0"));
    assert_eq!(
        std::fs::read_to_string(folder.join("background.js")).unwrap(),
        "v2"
    );
    assert!(!folder.join("images/icon.png").exists());
}

#[test]
fn preparing_leaves_out_hidden_files_and_the_extensions_own_tests() {
    let root = tempfile::tempdir().unwrap();
    let bundled = root.path().join("bundle");
    let data = root.path().join("data");
    write_extension(&bundled, "2.0.0", "v1");
    std::fs::write(bundled.join("cookies.test.js"), "test").unwrap();
    std::fs::write(bundled.join(".DS_Store"), "junk").unwrap();
    let folder = prepare_extension_folder(&bundled, &data).unwrap();
    assert!(folder.join("background.js").exists());
    assert!(!folder.join("cookies.test.js").exists());
    assert!(!folder.join(".DS_Store").exists());
    // And an unchanged bundle is still recognised as unchanged.
    let before = std::fs::metadata(folder.join("background.js"))
        .unwrap()
        .modified()
        .unwrap();
    prepare_extension_folder(&bundled, &data).unwrap();
    assert_eq!(
        std::fs::metadata(folder.join("background.js"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}

#[test]
fn preparing_a_missing_bundle_is_an_error_not_an_empty_folder() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        prepare_extension_folder(&root.path().join("nope"), &root.path().join("data")).is_err()
    );
    assert!(!root.path().join("data/ChromeExtension").exists());
}

fn status(version: &str, seen: SystemTime) -> ExtensionStatus {
    let unix = seen
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    ExtensionStatus {
        version: version.into(),
        last_seen: unix - 978_307_200.0,
    }
}

#[test]
fn the_state_follows_the_last_report() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let minutes = |m: u64| now - Duration::from_secs(m * 60);
    let table = [
        (Some("1.1.0"), None, ExtensionState::NotConnected),
        (
            Some("1.1.0"),
            Some(status("1.1.0", minutes(5))),
            ExtensionState::Connected,
        ),
        (
            Some("1.1.0"),
            Some(status("1.0.1", minutes(5))),
            ExtensionState::Outdated,
        ),
        (
            Some("1.1.0"),
            Some(status("1.1.0", minutes(180))),
            ExtensionState::NotConnected,
        ),
        (
            None,
            Some(status("1.1.0", minutes(5))),
            ExtensionState::Connected,
        ),
    ];
    for (bundled, reported, expected) in table {
        assert_eq!(extension_state(bundled, reported.as_ref(), now), expected);
    }
}
