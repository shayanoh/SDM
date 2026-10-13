/// WCAG 2.1 contrast-ratio math over sRGB hex colors, used to gate every
/// bundled theme's text-on-surface pairs at AA level (spec §10.1). Pure so
/// it is testable without touching any platform color type.
pub enum ContrastRatio {}

impl ContrastRatio {
    /// The WCAG contrast ratio between two `#RRGGBB` hex colors, from `1`
    /// (identical) to `21` (black on white).
    ///
    /// # Panics
    /// If either color is not six characters long after trimming
    /// whitespace and one leading `#` (Swift `precondition`).
    pub fn between(hex_a: &str, hex_b: &str) -> f64 {
        let luminance_a = relative_luminance(hex_a);
        let luminance_b = relative_luminance(hex_b);
        let lighter = luminance_a.max(luminance_b);
        let darker = luminance_a.min(luminance_b);
        (lighter + 0.05) / (darker + 0.05)
    }

    /// WCAG AA for normal text: a contrast ratio of at least 4.5:1.
    pub fn passes_aa(hex_a: &str, hex_b: &str) -> bool {
        Self::between(hex_a, hex_b) >= 4.5
    }
}

fn relative_luminance(hex: &str) -> f64 {
    let (r, g, b) = components(hex);
    fn linearize(channel: f64) -> f64 {
        if channel <= 0.03928 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
}

fn components(hex: &str) -> (f64, f64, f64) {
    // Foundation's `.whitespaces` is spaces and tabs (no newlines).
    let trimmed = hex.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r');
    let cleaned = trimmed.strip_prefix('#').unwrap_or(trimmed);
    assert!(
        cleaned.chars().count() == 6,
        "expected a #RRGGBB hex color, got {hex}"
    );
    // Like `Scanner.scanHexInt64`: an optional `0x`/`0X` prefix, then the
    // leading run of hex digits; a non-hex string leaves the value at 0.
    let digits = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
        .unwrap_or(cleaned);
    let run: String = digits
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    let value = u64::from_str_radix(&run, 16).unwrap_or(0);
    let r = ((value & 0xFF0000) >> 16) as f64 / 255.0;
    let g = ((value & 0x00FF00) >> 8) as f64 / 255.0;
    let b = (value & 0x0000FF) as f64 / 255.0;
    (r, g, b)
}
