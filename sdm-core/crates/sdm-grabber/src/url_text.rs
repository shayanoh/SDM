//! Foundation `URL` text accessors the grabber relies on, over
//! [`SourceUrl`].

use percent_encoding::percent_decode_str;
use sdm_core::SourceUrl;

/// Swift `URL.path`: the percent-decoded path (`""` when the URL has no
/// path, `/` for a bare root). Falls back to the text after the authority
/// for a string the `url` crate cannot parse.
pub(crate) fn decoded_path(url: &SourceUrl) -> String {
    match url.as_url() {
        Some(parsed) => percent_decode_str(parsed.path())
            .decode_utf8_lossy()
            .into_owned(),
        None => String::new(),
    }
}

/// Swift `URL.lastPathComponent`: the percent-decoded last non-empty path
/// segment, `/` for a root path and `""` for a URL with no path at all.
pub(crate) fn last_path_component(url: &SourceUrl) -> String {
    let path = match url.as_url() {
        Some(parsed) => parsed.path().to_owned(),
        None => return String::new(),
    };
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() {
            String::new()
        } else {
            "/".into()
        };
    }
    let last = trimmed.rsplit('/').next().unwrap_or("");
    percent_decode_str(last).decode_utf8_lossy().into_owned()
}

/// Swift `URL.host`: `None` when the URL has no (or an empty) host.
pub(crate) fn host(url: &SourceUrl) -> Option<String> {
    url.as_url()
        .and_then(|parsed| parsed.host_str())
        .map(str::to_owned)
}

/// Swift `URL.deletingLastPathComponent().path`: the percent-decoded path
/// with its last component removed. `""` for a URL with no path, `/..` for a
/// bare root path (what Foundation answers), `/` for a file directly under
/// the root. A trailing slash is not a component of its own (`/a/b/` gives
/// `/a`).
pub(crate) fn directory_path(url: &SourceUrl) -> String {
    let path = decoded_path(url);
    if path.is_empty() {
        return String::new();
    }
    if path == "/" {
        return "/..".into();
    }
    let trimmed = path.strip_suffix('/').unwrap_or(&path);
    match trimmed.rfind('/') {
        Some(0) | None => "/".into(),
        Some(index) => trimmed[..index].to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(url: &str) -> String {
        directory_path(&SourceUrl::new(url).unwrap())
    }

    /// Expected values were produced by Foundation's
    /// `URL.deletingLastPathComponent().path`.
    #[test]
    fn directory_path_matches_foundation() {
        assert_eq!(dir("https://h/a/b/file.zip"), "/a/b");
        assert_eq!(dir("https://h/file.zip"), "/");
        assert_eq!(dir("https://h/a/b/"), "/a");
        assert_eq!(dir("https://h/?v=1"), "/..");
        assert_eq!(dir("https://h/a%20b/c.zip"), "/a b");
        assert_eq!(dir("https://h/a//c.zip"), "/a/");
    }
}
