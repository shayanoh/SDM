use sdm_resolve::lzfse;

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[test]
fn lzfse_round_trips() {
    let original = pattern(200_000);
    let compressed = lzfse::encode(&original).unwrap();
    assert!(compressed.len() < original.len());
    assert_eq!(lzfse::decode(&compressed).unwrap(), original);
}

#[test]
fn lzfse_round_trips_shell_script() {
    let original = b"#!/bin/sh\necho hello\n".to_vec();
    assert_eq!(
        lzfse::decode(&lzfse::encode(&original).unwrap()).unwrap(),
        original
    );
}

#[test]
fn lzfse_rejects_garbage() {
    assert!(lzfse::decode(&[1, 2, 3, 4, 5, 6, 7, 8]).is_err());
}

/// `tests/fixtures/sample.lzfse` was made by macOS
/// `compression_tool -encode -a lzfse` from `sample.bin`; the decoder must
/// read what the real app's vendored blobs are made of.
#[test]
fn decodes_a_file_compressed_by_macos() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let plain = std::fs::read(dir.join("sample.bin")).unwrap();
    let compressed = std::fs::read(dir.join("sample.lzfse")).unwrap();
    assert_eq!(plain.len(), 200_000);
    assert_eq!(lzfse::decode(&compressed).unwrap(), plain);
}

/// The size hint only reserves capacity: a hint that is far too small or far
/// too large changes nothing about the decoded bytes.
#[test]
fn lzfse_decode_is_independent_of_the_size_hint() {
    let original = pattern(200_000);
    let compressed = lzfse::encode(&original).unwrap();
    for hint in [0, 10, original.len(), usize::MAX] {
        assert_eq!(
            lzfse::decode_with_size_hint(&compressed, hint).unwrap(),
            original
        );
    }
}
