//! Reducing a name that came from outside (a URL, a server header, a saved
//! file, the user) to one path component that cannot leave the folder it is
//! joined onto.
//!
//! `Path::join` with an absolute name replaces the base, and `..` walks out of
//! it, so every filename and package name must pass through here before it
//! meets a folder.

/// The name used when nothing usable is left, matching the grabber's own
/// fallback for a link with no name.
pub const FALLBACK_NAME: &str = "download";

/// The last component of `name`, or `None` when there is none to use.
///
/// Text up to the last `/` is dropped (a trailing `/` is ignored), NUL
/// characters are removed, and an empty result, `.` or `..` is refused. Any
/// ordinary name (spaces, Unicode, dots inside the name, a leading dot, long
/// names) comes back unchanged.
pub fn single_path_component(name: &str) -> Option<String> {
    let trimmed = name.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    let cleaned: String = if last.contains('\0') {
        last.chars().filter(|c| *c != '\0').collect()
    } else {
        last.to_owned()
    };
    match cleaned.as_str() {
        "" | "." | ".." => None,
        _ => Some(cleaned),
    }
}

/// [`single_path_component`], or [`FALLBACK_NAME`] when nothing is usable.
pub fn safe_path_component(name: &str) -> String {
    single_path_component(name).unwrap_or_else(|| FALLBACK_NAME.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_unchanged() {
        let long = format!("{}.bin", "a".repeat(200));
        for name in [
            "plain.bin",
            "with spaces.bin",
            "نام فایل.bin",
            "日本語.bin",
            "archive.tar.gz",
            "a.b.c.d",
            ".hidden",
            "...three",
            "trailing.",
            "name..with..dots",
            " leading space",
            long.as_str(),
        ] {
            assert_eq!(safe_path_component(name), name, "{name}");
        }
    }

    #[test]
    fn separators_dots_and_nul_are_reduced() {
        let table = [
            ("/Users/me/x", "x"),
            ("../../x", "x"),
            ("a/../b", "b"),
            ("dir/", "dir"),
            ("dir///", "dir"),
            ("x\0y", "xy"),
            ("", "download"),
            ("/", "download"),
            (".", "download"),
            ("..", "download"),
            ("a/..", "download"),
            ("\0", "download"),
        ];
        for (name, expected) in table {
            assert_eq!(safe_path_component(name), expected, "{name:?}");
        }
        assert_eq!(single_path_component(".."), None);
        assert_eq!(single_path_component("a/b"), Some("b".into()));
    }

    #[test]
    fn it_is_idempotent() {
        for name in ["/a/b", "..", "x\0", "ok.bin"] {
            let once = safe_path_component(name);
            assert_eq!(safe_path_component(&once), once);
        }
    }
}
