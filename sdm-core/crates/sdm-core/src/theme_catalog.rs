use crate::theme::Theme;

/// The bundled theme files, embedded at compile time. Spec §10.1: "adding a
/// theme is adding a JSON file": add the file and one line here.
const THEME_FILES: &[&str] = &[
    include_str!("../resources/themes/catppuccin-frappe.json"),
    include_str!("../resources/themes/catppuccin-latte.json"),
    include_str!("../resources/themes/catppuccin-macchiato.json"),
    include_str!("../resources/themes/catppuccin-mocha.json"),
    include_str!("../resources/themes/dark.json"),
    include_str!("../resources/themes/deep-purple.json"),
    include_str!("../resources/themes/dracula.json"),
    include_str!("../resources/themes/gruvbox.json"),
    include_str!("../resources/themes/light.json"),
    include_str!("../resources/themes/midnight-blue.json"),
    include_str!("../resources/themes/nord.json"),
    include_str!("../resources/themes/one-dark.json"),
    include_str!("../resources/themes/rose-pine.json"),
    include_str!("../resources/themes/solarized-dark.json"),
    include_str!("../resources/themes/solarized-light.json"),
    include_str!("../resources/themes/tokyo-night.json"),
];

/// Loads every bundled theme JSON file. Swift reads them from the module
/// resource bundle; here they are embedded with `include_str!`.
pub enum ThemeCatalog {}

impl ThemeCatalog {
    /// Every bundled theme that decodes, sorted by `name`. As in Swift, a
    /// malformed file is silently skipped rather than failing the catalog,
    /// and the sort is the only ordering (Swift's bundle enumeration order
    /// is unspecified, so the explicit `name` sort is what is observable).
    pub fn built_in_themes() -> Vec<Theme> {
        let mut themes: Vec<Theme> = THEME_FILES
            .iter()
            .filter_map(|json| serde_json::from_str::<Theme>(json).ok())
            .collect();
        themes.sort_by(|a, b| a.name.cmp(&b.name));
        themes
    }
}
