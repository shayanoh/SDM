use sdm_core::ContrastRatio;

#[test]
fn black_on_white_has_the_maximum_contrast_ratio() {
    let ratio = ContrastRatio::between("#000000", "#FFFFFF");
    assert!((ratio - 21.0).abs() < 0.01);
}

#[test]
fn identical_colors_have_a_ratio_of_one() {
    let ratio = ContrastRatio::between("#808080", "#808080");
    assert!((ratio - 1.0).abs() < 0.01);
}

#[test]
fn contrast_ratio_is_symmetric() {
    assert_eq!(
        ContrastRatio::between("#000000", "#FFFFFF"),
        ContrastRatio::between("#FFFFFF", "#000000")
    );
}

#[test]
fn black_on_white_passes_aa() {
    assert!(ContrastRatio::passes_aa("#000000", "#FFFFFF"));
}

#[test]
fn identical_grays_fail_aa() {
    assert!(!ContrastRatio::passes_aa("#808080", "#808080"));
}

#[test]
fn hex_without_a_leading_hash_is_accepted() {
    assert_eq!(
        ContrastRatio::between("000000", "FFFFFF"),
        ContrastRatio::between("#000000", "#FFFFFF")
    );
}
