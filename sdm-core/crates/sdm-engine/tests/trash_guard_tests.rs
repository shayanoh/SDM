//! Guards against a test ever reaching the user's real Trash.
//!
//! A test that built an engine without `with_trash` once moved files into
//! `~/.Trash` on every run, because the engine defaulted to `SystemTrash`. The
//! default now refuses, and these tests pin that and the rule that no test or
//! benchmark source names `SystemTrash`.
mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use sdm_core::{DownloadItem, DownloadPackage};
use sdm_engine::{DownloadEngine, FakeOrigin, InMemoryStateStore, RetryPolicy};
use support::{engine_settings, make_scratch_directory, test_payload, within};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_engine_built_without_a_trash_refuses_to_trash_anything() {
    let dir = make_scratch_directory();
    let engine = DownloadEngine::new(
        Arc::new(FakeOrigin::new(test_payload(10))),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 1, 8, dir.path()),
        RetryPolicy::new(),
        None,
        None,
        None,
    );
    let item = DownloadItem::http(
        sdm_core::SourceUrl::new("https://example.com/a.bin").unwrap(),
        "a.bin",
    );
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![item.clone()]))
        .await;
    within(engine.run_until_idle()).await.unwrap();
    let destination = dir.path().join("Batch").join("a.bin");
    assert!(destination.exists());

    engine.remove_item(item.id, true).await;

    // The refusing default left the file where it was.
    assert!(destination.exists());
}

fn rust_files_under(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files_under(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_test_or_benchmark_source_names_the_system_trash() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut roots = vec![crates.join("sdm-bench/src")];
    for entry in fs::read_dir(&crates).unwrap() {
        let tests = entry.unwrap().path().join("tests");
        if tests.is_dir() {
            roots.push(tests);
        }
    }
    let this_file = Path::new(file!()).file_name().unwrap().to_owned();
    let mut offenders = Vec::new();
    for root in roots {
        let mut files = Vec::new();
        rust_files_under(&root, &mut files);
        for file in files {
            if file.file_name() == Some(&this_file) {
                continue;
            }
            if fs::read_to_string(&file).unwrap().contains("SystemTrash") {
                offenders.push(file);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these sources name SystemTrash, which would move files into the real Trash: {offenders:?}"
    );
}
