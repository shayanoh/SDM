mod support;

use sdm_app::AppError;
use sdm_app::deletion::describe;
use sdm_app::protocol::{DeletionTarget, ItemStateDto};
use sdm_engine::FakeOrigin;
use support::*;

fn write(folder: &std::path::Path, name: &str, len: usize) {
    std::fs::write(folder.join(name), vec![0u8; len]).unwrap();
}

#[test]
fn sums_actual_sizes_of_output_and_incomplete_files() {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "a.bin", 100);
    write(folder.path(), "a.bin.incomplete", 40);

    let (total, names) = describe(folder.path(), &[("a.bin".into(), vec!["a.bin".into()])]);

    assert_eq!(total, 140.0);
    assert_eq!(names, ["a.bin", "a.bin.incomplete"]);
}

#[test]
fn part_files_are_included_and_names_are_sorted() {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "clip.f251.webm.incomplete", 7);
    write(folder.path(), "clip.f137.mp4", 5);
    write(folder.path(), "clip.mp4", 11);

    let (total, names) = describe(
        folder.path(),
        &[(
            "clip.mp4".into(),
            vec!["clip.f137.mp4".into(), "clip.f251.webm".into()],
        )],
    );

    assert_eq!(total, 23.0);
    assert_eq!(
        names,
        ["clip.f137.mp4", "clip.f251.webm.incomplete", "clip.mp4"]
    );
}

#[test]
fn missing_files_and_a_missing_folder_are_skipped() {
    let folder = tempfile::tempdir().unwrap();
    let files = [("gone.bin".to_owned(), vec!["gone.bin".to_owned()])];
    assert_eq!(describe(folder.path(), &files), (0.0, Vec::new()));
    assert_eq!(
        describe(&folder.path().join("nope"), &files),
        (0.0, Vec::new())
    );
}

#[test]
fn a_shared_name_is_counted_once() {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "a.bin", 10);
    let files = [
        ("a.bin".to_owned(), vec!["a.bin".to_owned()]),
        ("a.bin".to_owned(), vec!["a.bin".to_owned()]),
    ];
    let (total, names) = describe(folder.path(), &files);
    assert_eq!((total, names.len()), (10.0, 1));
}

async fn completed_item(h: &Harness, name: &str) -> (String, String) {
    h.app
        .add_url(&format!("https://example.com/{name}"), None)
        .await
        .unwrap();
    let ids = || {
        h.sink
            .patches()
            .iter()
            .rev()
            .find_map(|p| p.structure.clone())
            .map(|s| s.packages)
            .unwrap_or_default()
    };
    assert!(
        tick_until(&h.app, 600, || ids()
            .iter()
            .flat_map(|p| &p.items)
            .any(|i| i.filename == name && i.state == ItemStateDto::Completed))
        .await
    );
    let package = ids()
        .into_iter()
        .find(|p| p.items.iter().any(|i| i.filename == name))
        .unwrap();
    (package.id.clone(), package.items[0].id.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_files_the_title_names_the_deletion() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    let (package, item) = completed_item(&h, "x.bin").await;

    let by_item = h
        .app
        .describe_deletion(&DeletionTarget::Items {
            ids: vec![item.clone()],
        })
        .await
        .unwrap();
    assert_eq!(by_item.title, "Remove From List And Delete Files?");
    assert_eq!(by_item.total_bytes, 3000.0);
    assert_eq!(by_item.names, ["x.bin"]);

    let by_package = h
        .app
        .describe_deletion(&DeletionTarget::Package { id: package })
        .await
        .unwrap();
    assert_eq!(by_package, by_item);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_files_means_the_remove_from_list_title() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    let (_, item) = completed_item(&h, "y.bin").await;
    std::fs::remove_file(h.downloads.path().join("y.bin").join("y.bin")).unwrap();

    let info = h
        .app
        .describe_deletion(&DeletionTarget::Items { ids: vec![item] })
        .await
        .unwrap();

    assert_eq!(info.title, "Remove From List?");
    assert_eq!(info.total_bytes, 0.0);
    assert!(info.names.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_package_returns_the_package_title() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let info = h
        .app
        .describe_deletion(&DeletionTarget::Package {
            id: "00000000-0000-0000-0000-000000000000".into(),
        })
        .await
        .unwrap();
    assert_eq!(info.title, "Delete Package From Disk?");
    assert!(info.names.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_id_is_an_error() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let result = h
        .app
        .describe_deletion(&DeletionTarget::Items {
            ids: vec!["nope".into()],
        })
        .await;
    assert!(matches!(result, Err(AppError::InvalidArgument { .. })));
}
