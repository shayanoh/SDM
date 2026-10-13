use std::path::Path;

use sdm_engine::SparseFile;

#[test]
fn creates_incomplete_file_with_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("movie.mp4");

    let file = SparseFile::open(&final_path, 100).unwrap();

    let incomplete = dir.path().join("movie.mp4.incomplete");
    assert!(incomplete.exists());
    assert!(!final_path.exists());
    file.close();
}

#[test]
fn writes_land_at_their_absolute_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("out.bin");

    let file = SparseFile::open(&final_path, 10).unwrap();
    file.write(&[9, 9], 8).unwrap();
    file.write(&[1, 2, 3], 0).unwrap();
    let result = file.finalize().unwrap();

    assert_eq!(
        std::fs::read(result).unwrap(),
        vec![1, 2, 3, 0, 0, 0, 0, 0, 9, 9]
    );
}

#[test]
fn finalize_renames_off_the_incomplete_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("movie.mp4");

    let file = SparseFile::open(&final_path, 4).unwrap();
    file.write(&[1, 2, 3, 4], 0).unwrap();
    let result = file.finalize().unwrap();

    assert_eq!(result, final_path);
    assert!(final_path.exists());
    assert!(!dir.path().join("movie.mp4.incomplete").exists());
}

#[test]
fn reopening_preserves_existing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("out.bin");

    let first = SparseFile::open(&final_path, 6).unwrap();
    first.write(&[1, 2, 3], 0).unwrap();
    first.sync().unwrap();
    first.close();

    let second = SparseFile::open(&final_path, 6).unwrap();
    second.write(&[4, 5, 6], 3).unwrap();
    let result = second.finalize().unwrap();

    assert_eq!(std::fs::read(result).unwrap(), vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn incomplete_path_appends_suffix() {
    let path = Path::new("/tmp/a.bin");
    assert_eq!(
        SparseFile::incomplete_path(path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        "a.bin.incomplete"
    );
}
