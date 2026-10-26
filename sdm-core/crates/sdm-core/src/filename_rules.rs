//! Windows filename rules, as a pure function so every OS can test them.
//!
//! Windows rejects the characters `<>:"|?*` and control characters, treats `\`
//! as a separator, silently strips trailing dots and spaces, reserves the
//! device names `CON PRN AUX NUL COM1-9 LPT1-9` (with or without an extension),
//! and limits a path component to 255 UTF-16 units.

const MAX_UNITS: usize = 255;
const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

fn is_reserved_stem(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(digit) = upper.strip_prefix(prefix)
            && digit.len() == 1
            && matches!(digit.as_bytes()[0], b'1'..=b'9')
        {
            return true;
        }
    }
    false
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The last path component of `name` reduced to something Windows accepts, or
/// `None` when nothing usable is left. Splits on both `/` and `\`.
pub fn windows_safe_name(name: &str) -> Option<String> {
    let last = name
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    let replaced: String = last
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = replaced.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        return None;
    }

    let (stem, extension) = match trimmed.find('.') {
        Some(0) | None => (trimmed, ""),
        Some(index) => trimmed.split_at(index),
    };
    // `extension` keeps its leading dot. Truncate the stem first, then the
    // extension if it alone is too long, so the total stays within the limit.
    let mut stem = stem.to_owned();
    let mut extension = extension.to_owned();
    while units(&stem) + units(&extension) > MAX_UNITS && units(&stem) > 1 {
        stem.pop();
    }
    while units(&stem) + units(&extension) > MAX_UNITS && !extension.is_empty() {
        extension.pop();
    }

    let mut out = format!("{stem}{extension}");
    if is_reserved_stem(&stem) {
        out.insert(0, '_');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, expected). `None` means nothing usable is left.
    const CASES: &[(&str, Option<&str>)] = &[
        ("plain.bin", Some("plain.bin")),
        ("with spaces.bin", Some("with spaces.bin")),
        ("نام فایل.bin", Some("نام فایل.bin")),
        ("a<b>c:d\"e|f?g*h.txt", Some("a_b_c_d_e_f_g_h.txt")),
        ("tab\there.txt", Some("tab_here.txt")),
        ("trailing dot.", Some("trailing dot")),
        ("trailing space ", Some("trailing space")),
        ("trailing. . ", Some("trailing")),
        ("CON", Some("_CON")),
        ("con.txt", Some("_con.txt")),
        ("Nul.tar.gz", Some("_Nul.tar.gz")),
        ("COM1", Some("_COM1")),
        ("lpt9.log", Some("_lpt9.log")),
        ("COM0.txt", Some("COM0.txt")),
        ("console.txt", Some("console.txt")),
        ("folder\\evil.txt", Some("evil.txt")),
        ("a/b\\c.txt", Some("c.txt")),
        ("..", None),
        (".", None),
        ("", None),
        ("   ", None),
        ("...", None),
    ];

    #[test]
    fn fixture_table() {
        for (input, expected) in CASES {
            assert_eq!(
                windows_safe_name(input).as_deref(),
                *expected,
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn long_names_are_truncated_keeping_the_extension() {
        let long = format!("{}.mkv", "a".repeat(400));
        let out = windows_safe_name(&long).unwrap();
        assert!(out.encode_utf16().count() <= 255, "{}", out.len());
        assert!(out.ends_with(".mkv"));
    }

    #[test]
    fn a_huge_extension_cannot_defeat_the_limit() {
        let long = format!("name.{}", "b".repeat(400));
        let out = windows_safe_name(&long).unwrap();
        assert!(out.encode_utf16().count() <= 255);
    }
}
