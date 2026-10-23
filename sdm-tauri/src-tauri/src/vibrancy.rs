//! Mica on Windows 11, Acrylic on Windows 10: the translucent window effect
//! behind the sidebar and toolbar. Windows only; other platforms keep solid
//! theme colors.

/// A translucent window backdrop.
// Only the Windows build applies an effect; elsewhere only the tests use these.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Windows 11.
    Mica,
    /// Windows 10 (1809 and later) and Windows 11.
    Acrylic,
}

/// Best first: Mica where the OS has it, else Acrylic.
#[cfg_attr(not(windows), allow(dead_code))]
pub const PREFERENCE: [Effect; 2] = [Effect::Mica, Effect::Acrylic];

#[cfg_attr(not(windows), allow(dead_code))]
/// Tries each effect in `order` until `apply` reports one took; nothing is
/// tried after a success.
pub fn first_that_applies(
    order: &[Effect],
    mut apply: impl FnMut(Effect) -> bool,
) -> Option<Effect> {
    order.iter().copied().find(|effect| apply(*effect))
}

/// Applies the best effect the OS supports to `window` and says which took, or
/// `None` (logged, never fatal) when the window stays opaque.
#[cfg(windows)]
pub fn apply(window: &tauri::WebviewWindow) -> Option<Effect> {
    // Acrylic's tint is only used on older Windows 10 builds; a dark, mostly
    // transparent one keeps the sidebar legible there.
    const ACRYLIC_TINT: (u8, u8, u8, u8) = (18, 18, 18, 125);
    let applied = first_that_applies(&PREFERENCE, |effect| {
        let outcome = match effect {
            Effect::Mica => window_vibrancy::apply_mica(window, None),
            Effect::Acrylic => window_vibrancy::apply_acrylic(window, Some(ACRYLIC_TINT)),
        };
        if let Err(error) = &outcome {
            tracing::info!("{effect:?} is not available here: {error}");
        }
        outcome.is_ok()
    });
    if applied.is_none() {
        tracing::warn!("no window effect applied; the window stays opaque");
    }
    applied
}

/// Other platforms keep solid theme colors.
#[cfg(not(windows))]
pub fn apply(_window: &tauri::WebviewWindow) -> Option<Effect> {
    None
}

/// Whether a window effect is in force, for the frontend to read once.
pub struct VibrancyState(pub Option<Effect>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_is_preferred_when_it_applies() {
        let mut tried = Vec::new();
        let applied = first_that_applies(&PREFERENCE, |effect| {
            tried.push(effect);
            true
        });
        assert_eq!(applied, Some(Effect::Mica));
        assert_eq!(tried, [Effect::Mica], "nothing is tried after a success");
    }

    #[test]
    fn acrylic_is_the_fallback_for_windows_10() {
        let applied = first_that_applies(&PREFERENCE, |effect| effect == Effect::Acrylic);
        assert_eq!(applied, Some(Effect::Acrylic));
    }

    #[test]
    fn no_effect_when_none_applies() {
        assert_eq!(first_that_applies(&PREFERENCE, |_| false), None);
        assert_eq!(first_that_applies(&[], |_| true), None);
    }
}
