//! The bundled themes as CSS custom properties, so the frontend never holds a
//! color literal: every color in a component is `var(--role)`.

use sdm_core::Theme;

/// A color value that is safe inside a declaration: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, or an `rgb(...)`/`rgba(...)` of digits, commas, dots, percent
/// signs and spaces. Anything else becomes `transparent`, so a bad theme file
/// can never close the declaration or add another one.
fn css_color(value: &str) -> String {
    let v = value.trim();
    let hex = v.strip_prefix('#').is_some_and(|h| {
        matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    });
    let function = ["rgb(", "rgba("].iter().any(|p| v.starts_with(p))
        && v.ends_with(')')
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | ',' | '.' | '%' | ' '));
    if hex || function {
        v.to_owned()
    } else {
        "transparent".to_owned()
    }
}

/// `:root { --surface-primary: #...; ... }` for `theme`, plus `color-scheme`.
pub fn theme_css(theme: &Theme) -> String {
    let roles: [(&str, &str); 15] = [
        ("surface-primary", &theme.surface_primary),
        ("surface-secondary", &theme.surface_secondary),
        ("surface-tertiary", &theme.surface_tertiary),
        ("text-primary", &theme.text_primary),
        ("text-secondary", &theme.text_secondary),
        ("text-tertiary", &theme.text_tertiary),
        ("accent", &theme.accent),
        ("border", &theme.border),
        ("status-online", &theme.status_online),
        ("status-faulty", &theme.status_faulty),
        ("status-offline", &theme.status_offline),
        ("status-failed", &theme.status_failed),
        ("progress-fill", &theme.progress_fill),
        ("completed-segment-fill", &theme.completed_segment_fill),
        ("active-head-tint", &theme.active_head_tint),
    ];
    let extra: [(&str, &str); 4] = [
        ("graph-stroke", &theme.graph_stroke),
        ("graph-average-stroke", &theme.graph_average_stroke),
        ("sidebar-background", &theme.sidebar_background),
        ("selection-tint", &theme.selection_tint),
    ];
    let mut css = String::from(":root {\n");
    for (name, value) in roles.iter().chain(extra.iter()) {
        css.push_str(&format!("  --{name}: {};\n", css_color(value)));
    }
    css.push_str(&format!(
        "  color-scheme: {};\n}}\n",
        if theme.is_dark { "dark" } else { "light" }
    ));
    css
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::ThemeCatalog;

    #[test]
    fn every_bundled_theme_produces_all_roles() {
        for theme in ThemeCatalog::built_in_themes() {
            let css = theme_css(&theme);
            for role in [
                "surface-primary",
                "text-primary",
                "accent",
                "progress-fill",
                "completed-segment-fill",
                "selection-tint",
            ] {
                assert!(
                    css.contains(&format!("--{role}: #")),
                    "{}: missing {role}",
                    theme.id
                );
            }
            assert!(css.contains("color-scheme:"));
        }
    }

    #[test]
    fn a_hostile_value_cannot_close_the_declaration() {
        let mut theme = ThemeCatalog::built_in_themes().remove(0);
        theme.accent = "#fff; } body { display:none".into();
        let css = theme_css(&theme);
        assert_eq!(css.matches('}').count(), 1, "{css}");
        assert!(!css.contains("display"), "{css}");
        assert!(css.contains("--accent: transparent;"), "{css}");
    }
}
