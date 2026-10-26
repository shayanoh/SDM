//! The tray icon's pixels. A port of `MenuBarRingIcon`: the SDM logo, solid
//! when nothing runs; while downloads run, the whole logo dimmed to 25% with
//! the left part, as wide as the overall progress, at full opacity.
//!
//! The logo is a white silhouette, so only its alpha matters: the color comes
//! from the tint.

/// How much of its opacity the logo keeps while downloads run (25%).
const DIM_NUMERATOR: u32 = 1;
const DIM_DENOMINATOR: u32 = 4;

/// The icon's color. macOS tints a template image itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // each variant is chosen on some platforms only
pub enum Tint {
    White,
    Black,
    Template,
}

impl Tint {
    fn value(self) -> u8 {
        match self {
            Tint::Black => 0,
            Tint::White | Tint::Template => 255,
        }
    }
}

/// The icon for a state. `logo_rgba` is the logo's RGBA pixels (`width` by
/// `height`); only its alpha is used. `fraction` is overall progress (0 to 1;
/// anything else is clamped, a NaN counts as 0).
pub fn compose(
    logo_rgba: &[u8],
    width: u32,
    height: u32,
    running: bool,
    fraction: f64,
    tint: Tint,
) -> Vec<u8> {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled_columns = fraction * f64::from(width);
    let color = tint.value();
    let mut out = Vec::with_capacity(logo_rgba.len());
    for (index, pixel) in logo_rgba.as_chunks::<4>().0.iter().enumerate() {
        let column = index as u32 % width.max(1);
        let alpha = u32::from(pixel[3]);
        let alpha = if running && f64::from(column + 1) > filled_columns {
            (alpha * DIM_NUMERATOR * 2 + DIM_DENOMINATOR) / (DIM_DENOMINATOR * 2)
        } else {
            alpha
        };
        out.extend_from_slice(&[color, color, color, alpha as u8]);
    }
    debug_assert_eq!(height as usize * width as usize * 4, out.len());
    out
}

/// Shrinks `rgba` to `new_width` by `new_height` by averaging the alpha of
/// each block of source pixels (the color is not kept: the tint replaces it).
pub fn downscale(rgba: &[u8], width: u32, height: u32, new_width: u32, new_height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((new_width * new_height * 4) as usize);
    for y in 0..new_height {
        let (y0, y1) = (
            y * height / new_height,
            ((y + 1) * height / new_height).max(y * height / new_height + 1),
        );
        for x in 0..new_width {
            let (x0, x1) = (
                x * width / new_width,
                ((x + 1) * width / new_width).max(x * width / new_width + 1),
            );
            let mut sum = 0u32;
            let mut count = 0u32;
            for sy in y0..y1.min(height) {
                for sx in x0..x1.min(width) {
                    sum += u32::from(rgba[((sy * width + sx) * 4 + 3) as usize]);
                    count += 1;
                }
            }
            let alpha = (sum + count / 2).checked_div(count).unwrap_or(0);
            out.extend_from_slice(&[255, 255, 255, alpha as u8]);
        }
    }
    out
}

/// Windows: black on a light taskbar (`SystemUsesLightTheme` = 1), white
/// otherwise, including when the setting cannot be read.
#[cfg(any(windows, test))]
pub fn windows_tint(system_uses_light_theme: Option<u32>) -> Tint {
    match system_uses_light_theme {
        Some(1) => Tint::Black,
        _ => Tint::White,
    }
}

/// Linux: GNOME's top bar is dark whatever the scheme, so white there;
/// elsewhere black only on an explicit light scheme.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
pub fn linux_tint(desktop: &str, color_scheme: &str) -> Tint {
    if desktop.to_ascii_uppercase().contains("GNOME") {
        return Tint::White;
    }
    if color_scheme.contains("prefer-light") {
        Tint::Black
    } else {
        Tint::White
    }
}

/// The tint for the system's current setting.
pub fn system_tint() -> Tint {
    #[cfg(target_os = "macos")]
    {
        Tint::Template
    }
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let value = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
            .ok();
        windows_tint(value)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let scheme = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .ok()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .trim_matches('\'')
                    .to_owned()
            })
            .unwrap_or_default();
        linux_tint(&desktop, &scheme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-row logo of opaque white pixels.
    fn row(alphas: &[u8]) -> Vec<u8> {
        alphas.iter().flat_map(|a| [255, 255, 255, *a]).collect()
    }

    fn alphas(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks(4).map(|px| px[3]).collect()
    }

    #[test]
    fn idle_keeps_the_logo_as_it_is() {
        let out = compose(&row(&[255, 128, 0, 255]), 4, 1, false, 0.7, Tint::White);
        assert_eq!(alphas(&out), vec![255, 128, 0, 255]);
    }

    #[test]
    fn running_dims_everything_then_fills_from_the_left() {
        let logo = row(&[255, 255, 255, 255]);
        assert_eq!(
            alphas(&compose(&logo, 4, 1, true, 0.0, Tint::White)),
            vec![64, 64, 64, 64]
        );
        assert_eq!(
            alphas(&compose(&logo, 4, 1, true, 0.5, Tint::White)),
            vec![255, 255, 64, 64]
        );
        assert_eq!(
            alphas(&compose(&logo, 4, 1, true, 1.0, Tint::White)),
            vec![255, 255, 255, 255]
        );
    }

    #[test]
    fn a_partly_transparent_pixel_scales_with_the_dim() {
        let out = compose(&row(&[128, 128]), 2, 1, true, 0.5, Tint::White);
        assert_eq!(alphas(&out), vec![128, 32]);
    }

    #[test]
    fn progress_that_is_not_a_number_or_out_of_range_is_clamped() {
        let logo = row(&[255, 255]);
        assert_eq!(
            alphas(&compose(&logo, 2, 1, true, f64::NAN, Tint::White)),
            vec![64, 64]
        );
        assert_eq!(
            alphas(&compose(&logo, 2, 1, true, -3.0, Tint::White)),
            vec![64, 64]
        );
        assert_eq!(
            alphas(&compose(&logo, 2, 1, true, 9.0, Tint::White)),
            vec![255, 255]
        );
    }

    #[test]
    fn the_tint_sets_the_color() {
        let logo = row(&[255]);
        assert_eq!(
            &compose(&logo, 1, 1, false, 0.0, Tint::White)[..3],
            &[255, 255, 255]
        );
        assert_eq!(
            &compose(&logo, 1, 1, false, 0.0, Tint::Black)[..3],
            &[0, 0, 0]
        );
        // macOS tints a template image itself: it is drawn white.
        assert_eq!(
            &compose(&logo, 1, 1, false, 0.0, Tint::Template)[..3],
            &[255, 255, 255]
        );
    }

    #[test]
    fn downscaling_averages_the_alpha_of_each_block() {
        // 4x2 -> 2x1: blocks of 2x2.
        let logo: Vec<u8> = [[255, 255, 0, 0], [255, 255, 0, 0]]
            .iter()
            .flat_map(|alphas| row(alphas))
            .collect();
        let out = downscale(&logo, 4, 2, 2, 1);
        assert_eq!(alphas(&out), vec![255, 0]);
        let half: Vec<u8> = [[255, 0, 255, 0], [0, 255, 0, 255]]
            .iter()
            .flat_map(|alphas| row(alphas))
            .collect();
        assert_eq!(alphas(&downscale(&half, 4, 2, 2, 1)), vec![128, 128]);
    }

    #[test]
    fn windows_follows_the_taskbar_theme_and_defaults_to_white() {
        assert_eq!(windows_tint(Some(1)), Tint::Black);
        assert_eq!(windows_tint(Some(0)), Tint::White);
        assert_eq!(windows_tint(None), Tint::White);
    }

    #[test]
    fn linux_is_white_on_gnome_and_follows_the_scheme_elsewhere() {
        // GNOME's top bar is dark whatever the app scheme is.
        assert_eq!(linux_tint("GNOME", "default"), Tint::White);
        assert_eq!(linux_tint("ubuntu:GNOME", "prefer-light"), Tint::White);
        assert_eq!(linux_tint("KDE", "prefer-light"), Tint::Black);
        assert_eq!(linux_tint("KDE", "prefer-dark"), Tint::White);
        assert_eq!(linux_tint("sway", ""), Tint::White);
    }
}
