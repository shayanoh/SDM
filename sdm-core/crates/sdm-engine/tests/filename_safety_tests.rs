//! Filenames and package names never place a file outside the download
//! folder, and ordinary names are left exactly as they are.
mod support;

use sdm_core::{DownloadItem, DownloadPackage, SourceUrl};
use sdm_engine::FakeOrigin;
use support::{engine_settings, make_scratch_directory, new_engine, test_payload, within};

fn item(name: &str) -> DownloadItem {
    DownloadItem::http(SourceUrl::new("https://example.com/f").unwrap(), name)
}

/// Downloads `names` in package `package` and returns the folder the engine
/// was given, so a test can look at where the files landed.
async fn download(package: &str, names: &[&str]) -> tempfile::TempDir {
    let dir = make_scratch_directory();
    let engine = new_engine(
        FakeOrigin::new(test_payload(64)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let items = names.iter().map(|name| item(name)).collect();
    engine
        .add(DownloadPackage::new(package).with_items(items))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_names_land_at_exactly_the_name_given() {
    let long = format!("{}.bin", "a".repeat(200));
    let names = [
        "plain.bin",
        "name with spaces.bin",
        "نام فایل.bin",
        "日本語のファイル.bin",
        "archive.tar.gz",
        "many.dots.in.a.name.v1.2.3.bin",
        ".hidden",
        "UPPER Case.BIN",
        "dash-and_underscore (1).bin",
        long.as_str(),
    ];
    let dir = download("Ordinary Pack", &names).await;
    for name in names {
        let path = dir.path().join("Ordinary Pack").join(name);
        assert!(path.is_file(), "{name} should be at {}", path.display());
    }
    let package = download("بسته فارسی", &["a.bin"]).await;
    assert!(package.path().join("بسته فارسی/a.bin").is_file());
}

fn files_under(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Runs a download of one item named `name` in package `package`, with the
/// download folder one level inside a scratch directory, and a second
/// scratch directory standing in for "somewhere else the user can write".
/// Returns (download folder, scratch parent, elsewhere).
async fn attack(
    package: &str,
    name: &str,
) -> (std::path::PathBuf, tempfile::TempDir, tempfile::TempDir) {
    let parent = make_scratch_directory();
    let elsewhere = make_scratch_directory();
    let downloads = parent.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let engine = new_engine(
        FakeOrigin::new(test_payload(64)),
        engine_settings(1, 1, 8, &downloads),
    );
    engine
        .add(DownloadPackage::new(package).with_items(vec![item(name)]))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    (downloads, parent, elsewhere)
}

fn assert_contained(downloads: &std::path::Path, parent: &tempfile::TempDir) {
    let inside = files_under(downloads);
    assert_eq!(inside.len(), 1, "one file inside the folder: {inside:?}");
    // Nothing beside the download folder in its parent either.
    let outside: Vec<_> = files_under(parent.path())
        .into_iter()
        .filter(|path| !path.starts_with(downloads))
        .collect();
    assert!(outside.is_empty(), "escaped the folder: {outside:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_absolute_filename_stays_inside_the_download_folder() {
    // Built from a real directory so a failure shows the file there.
    let elsewhere = make_scratch_directory();
    let absolute = elsewhere.path().join("escaped.bin");
    let (downloads, parent, _unused) = attack("P", absolute.to_str().unwrap()).await;
    assert_contained(&downloads, &parent);
    assert!(
        files_under(elsewhere.path()).is_empty(),
        "an absolute name wrote outside the folder"
    );
    assert!(downloads.join("P/escaped.bin").is_file());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dot_dot_filenames_stay_inside_the_download_folder() {
    for name in ["../escaped.bin", "../../escaped.bin", "a/../../escaped.bin"] {
        let (downloads, parent, _elsewhere) = attack("P", name).await;
        assert_contained(&downloads, &parent);
        assert!(
            downloads.join("P/escaped.bin").is_file(),
            "{name} should land as P/escaped.bin"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_filename_that_is_only_dots_or_empty_falls_back_to_download() {
    for name in ["..", ".", "/", "a/.."] {
        let (downloads, parent, _elsewhere) = attack("P", name).await;
        assert_contained(&downloads, &parent);
        assert!(
            downloads.join("P/download").is_file(),
            "{name:?} should land as P/download"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn package_names_cannot_escape_the_download_folder() {
    let elsewhere = make_scratch_directory();
    let absolute = elsewhere.path().join("Pkg");
    for package in ["../Pkg", "../../Pkg", absolute.to_str().unwrap()] {
        let (downloads, parent, _unused) = attack(package, "a.bin").await;
        assert_contained(&downloads, &parent);
        assert!(downloads.join("Pkg/a.bin").is_file(), "{package}");
        assert!(files_under(elsewhere.path()).is_empty(), "{package}");
    }
}
