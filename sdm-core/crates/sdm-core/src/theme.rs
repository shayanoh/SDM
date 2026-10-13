/// One of spec §10.1's ~20 semantic roles, resolved to a color per theme.
/// Views read roles, never literal colors, so a new theme is a new JSON
/// file with no view code touched. Colors are `#RRGGBB` hex strings rather
/// than a platform color type, keeping this pure and dependency-free like
/// every other `sdm-core` model; the app layer bridges each field to a
/// platform color.
///
/// Text-tier colors (`text_primary`/`text_secondary`/`text_tertiary`) are
/// deliberately shared across every dark-appearance theme, and separately
/// across every light-appearance theme, rather than one independent set per
/// palette (see the bundled JSON under `resources/themes/` for the values
/// and the WCAG-AA contrast test for why): a palette's own "muted"/"comment"
/// color is often tuned for a code editor's de-emphasis, not spec §10.1's AA
/// gate.
///
/// Serialized with camelCase keys, as Swift's synthesized `Codable` does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub is_dark: bool,
    /// Attribution for this theme's core palette, e.g. "Nord (MIT)" or
    /// "SDM" for the four built-ins that are not sourced from anywhere
    /// else. Spec §10.1: "each carries its license attribution."
    pub source: String,

    pub surface_primary: String,
    pub surface_secondary: String,
    pub surface_tertiary: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_tertiary: String,
    pub accent: String,
    pub border: String,
    pub status_online: String,
    pub status_faulty: String,
    pub status_offline: String,
    pub status_failed: String,
    pub progress_fill: String,
    pub completed_segment_fill: String,
    pub active_head_tint: String,
    pub graph_stroke: String,
    pub graph_average_stroke: String,
    pub sidebar_background: String,
    pub selection_tint: String,
}

impl Theme {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        is_dark: bool,
        source: impl Into<String>,
        surface_primary: impl Into<String>,
        surface_secondary: impl Into<String>,
        surface_tertiary: impl Into<String>,
        text_primary: impl Into<String>,
        text_secondary: impl Into<String>,
        text_tertiary: impl Into<String>,
        accent: impl Into<String>,
        border: impl Into<String>,
        status_online: impl Into<String>,
        status_faulty: impl Into<String>,
        status_offline: impl Into<String>,
        status_failed: impl Into<String>,
        progress_fill: impl Into<String>,
        completed_segment_fill: impl Into<String>,
        active_head_tint: impl Into<String>,
        graph_stroke: impl Into<String>,
        graph_average_stroke: impl Into<String>,
        sidebar_background: impl Into<String>,
        selection_tint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            is_dark,
            source: source.into(),
            surface_primary: surface_primary.into(),
            surface_secondary: surface_secondary.into(),
            surface_tertiary: surface_tertiary.into(),
            text_primary: text_primary.into(),
            text_secondary: text_secondary.into(),
            text_tertiary: text_tertiary.into(),
            accent: accent.into(),
            border: border.into(),
            status_online: status_online.into(),
            status_faulty: status_faulty.into(),
            status_offline: status_offline.into(),
            status_failed: status_failed.into(),
            progress_fill: progress_fill.into(),
            completed_segment_fill: completed_segment_fill.into(),
            active_head_tint: active_head_tint.into(),
            graph_stroke: graph_stroke.into(),
            graph_average_stroke: graph_average_stroke.into(),
            sidebar_background: sidebar_background.into(),
            selection_tint: selection_tint.into(),
        }
    }
}
