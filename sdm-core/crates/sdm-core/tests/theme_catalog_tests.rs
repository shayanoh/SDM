use std::collections::HashSet;
use std::fs;
use std::path::Path;

use sdm_core::{ContrastRatio, ThemeCatalog};

#[test]
fn built_in_themes_includes_light_and_dark() {
    let themes = ThemeCatalog::built_in_themes();
    assert!(themes.iter().any(|t| t.id == "light"));
    assert!(themes.iter().any(|t| t.id == "dark"));
}

#[test]
fn every_built_in_theme_has_a_unique_id() {
    let ids: Vec<String> = ThemeCatalog::built_in_themes()
        .into_iter()
        .map(|t| t.id)
        .collect();
    let unique: HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn light_theme_is_not_dark_and_dark_theme_is_dark() {
    let themes = ThemeCatalog::built_in_themes();
    assert_eq!(
        themes.iter().find(|t| t.id == "light").map(|t| t.is_dark),
        Some(false)
    );
    assert_eq!(
        themes.iter().find(|t| t.id == "dark").map(|t| t.is_dark),
        Some(true)
    );
}

#[test]
fn there_are_sixteen_built_in_themes() {
    assert_eq!(ThemeCatalog::built_in_themes().len(), 16);
}

/// Spec §10.1: "A test asserts WCAG AA contrast for every text-on-surface
/// pair in every bundled theme, so an attractive palette cannot ship
/// unreadable secondary text."
#[test]
fn every_built_in_theme_passes_wcag_aa_for_all_text_surface_pairs() {
    for theme in ThemeCatalog::built_in_themes() {
        let text_colors = [
            &theme.text_primary,
            &theme.text_secondary,
            &theme.text_tertiary,
        ];
        let surface_colors = [
            &theme.surface_primary,
            &theme.surface_secondary,
            &theme.surface_tertiary,
        ];
        for text in text_colors {
            for surface in surface_colors {
                assert!(
                    ContrastRatio::passes_aa(text, surface),
                    "{}: {} on {} is below WCAG AA (ratio {})",
                    theme.id,
                    text,
                    surface,
                    ContrastRatio::between(text, surface)
                );
            }
        }
    }
}

// Rust-only: the embedded theme files must be byte-identical to the
// Swift-written fixtures.
#[test]
fn bundled_theme_files_are_byte_identical_to_the_swift_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let resources = root.join("resources/themes");
    let fixtures = root.join("../../fixtures/swift/themes");
    let mut names: Vec<_> = fs::read_dir(&resources)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names.len(), 16);
    for name in &names {
        let ours = fs::read(resources.join(name)).unwrap();
        let theirs = fs::read(fixtures.join(name)).unwrap();
        assert_eq!(ours, theirs, "{name:?} differs from the Swift fixture");
    }
    assert_eq!(fs::read_dir(&fixtures).unwrap().count(), names.len());
}
