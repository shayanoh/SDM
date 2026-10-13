//! Foundation `URL` text accessors the grabber relies on, over
//! [`SourceUrl`].

use percent_encoding::percent_decode_str;
use sdm_core::SourceUrl;

/// Swift `URL.path`: the percent-decoded path (`""` when the URL has no
/// path, `/` for a bare root). Falls back to the text after the authority
/// for a string the `url` crate cannot parse.
pub(crate) fn decoded_path(url: &SourceUrl) -> String {
    match url.as_url() {
        Some(_) if written_without_path(url) => String::new(),
        Some(parsed) => percent_decode_str(parsed.path())
            .decode_utf8_lossy()
            .into_owned(),
        None => String::new(),
    }
}

/// Swift `URL.lastPathComponent`: the percent-decoded last non-empty path
/// segment, `/` for a root path and `""` for a URL with no path at all.
pub(crate) fn last_path_component(url: &SourceUrl) -> String {
    // The last component of the *decoded* path, as Foundation does: an
    // encoded slash (`%2F`) is a separator once decoded, so the name can never
    // carry one.
    let path = decoded_path(url);
    if path.is_empty() {
        return String::new();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".into();
    }
    trimmed.rsplit('/').next().unwrap_or("").to_owned()
}

/// True when the text has an authority that runs straight into `?`, `#` or
/// the end (`https://h?x=1`). The `url` crate then reports the path `/`;
/// Foundation reports none.
fn written_without_path(url: &SourceUrl) -> bool {
    let text = url.as_str();
    let Some((_, after_scheme)) = text.split_once("://") else {
        return false;
    };
    match after_scheme.find(['/', '?', '#']) {
        Some(index) => !after_scheme[index..].starts_with('/'),
        None => false,
    }
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
    fn query_only_url_without_a_slash_has_no_path() {
        // The `url` crate reports path "/" for these; Foundation has none.
        let url = SourceUrl::new("https://h?x=1").unwrap();
        assert_eq!(last_path_component(&url), "");
        assert_eq!(decoded_path(&url), "");
        assert_eq!(directory_path(&url), "");
        let fragment = SourceUrl::new("https://h#frag").unwrap();
        assert_eq!(last_path_component(&fragment), "");
        // With the slash spelled out the root path stays.
        let slash = SourceUrl::new("https://h/?x=1").unwrap();
        assert_eq!(last_path_component(&slash), "/");
    }

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
