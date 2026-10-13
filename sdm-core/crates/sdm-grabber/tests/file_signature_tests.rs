use sdm_grabber::FileSignature;

#[test]
fn detects_zip_magic_bytes() {
    assert_eq!(
        FileSignature::detect(&[0x50, 0x4B, 0x03, 0x04]),
        FileSignature::Zip
    );
}

#[test]
fn detects_rar_magic_bytes() {
    assert_eq!(
        FileSignature::detect(&[0x52, 0x61, 0x72, 0x21]),
        FileSignature::Rar
    );
}

#[test]
fn detects_mp4_by_ftyp_box_at_offset_four() {
    let mut bytes = vec![0x00, 0x00, 0x00, 0x18];
    bytes.extend_from_slice(b"ftypmp42");
    assert_eq!(FileSignature::detect(&bytes), FileSignature::Mp4);
}

#[test]
fn detects_html_by_doctype() {
    let data = b"<!DOCTYPE html><html><body>Not found</body></html>";
    assert_eq!(FileSignature::detect(data), FileSignature::Html);
}

#[test]
fn unknown_for_unrecognized_bytes() {
    assert_eq!(
        FileSignature::detect(&[0x01, 0x02, 0x03]),
        FileSignature::Unknown
    );
}

#[test]
fn matches_accepts_expected_extension_pairs() {
    assert!(FileSignature::Zip.matches_extension("zip"));
    assert!(FileSignature::Mp4.matches_extension("mp4"));
    assert!(FileSignature::Mp4.matches_extension("m4v"));
}

#[test]
fn matches_rejects_contradicting_extension() {
    assert!(!FileSignature::Html.matches_extension("mp4"));
    assert!(!FileSignature::Zip.matches_extension("pdf"));
}

#[test]
fn unknown_signature_never_contradicts_an_extension() {
    // Insufficient data to sniff should never itself trigger a "faulty" verdict.
    assert!(FileSignature::Unknown.matches_extension("mp4"));
}
