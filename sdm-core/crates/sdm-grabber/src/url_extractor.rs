use std::collections::HashSet;

use linkify::{LinkFinder, LinkKind};
use sdm_core::SourceUrl;

use crate::url_text;

/// Pulls `http`/`https` links out of arbitrary pasted or dropped text.
/// Spec §7.1: a link detector rather than a hand-written regex, so URLs
/// embedded in prose and wrapped by ordinary line breaks are found
/// correctly. Swift uses `NSDataDetector`; this port uses the `linkify`
/// crate. Like `NSDataDetector`, it also finds a link written without a
/// scheme (`example.com/file.zip`) and reads it as `http://`.
pub enum UrlExtractor {}

impl UrlExtractor {
    /// Distinct grabbable links in order of first appearance. Distinctness is
    /// by the URL string exactly as written.
    pub fn extract_links(text: &str) -> Vec<SourceUrl> {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url]);
        finder.url_must_have_scheme(false);

        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();
        for link in finder.links(text) {
            let written = link.as_str();
            let spelled = if Self::has_scheme(written) {
                written.to_owned()
            } else {
                format!("http://{written}")
            };
            let Ok(url) = SourceUrl::new(&spelled) else {
                continue;
            };
            if !Self::is_grabbable(&url) {
                continue;
            }
            if seen.insert(url.as_str().to_owned()) {
                result.push(url);
            }
        }
        result
    }

    /// Whether a detected link was written with its scheme (`https://…`), as
    /// opposed to a bare `example.com/path`.
    fn has_scheme(link: &str) -> bool {
        link.split_once("://").is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
    }

    /// Whether a URL is worth adding to the grabber: an `http`/`https` URL
    /// that actually points *at* something. A bare host with no path (or
    /// just `/`) and no query — `something.com`, `https://something.com/`,
    /// or a dotted token like a version string that a link detector
    /// mistook for a link — is not a download and is dropped. A real path
    /// segment (`/file.zip`, `/watch`, `/@channel`) or any query string
    /// keeps it.
    pub fn is_grabbable(url: &SourceUrl) -> bool {
        let Some(parsed) = url.as_url() else {
            return false;
        };
        let scheme = parsed.scheme().to_lowercase();
        if scheme != "http" && scheme != "https" {
            return false;
        }
        if url_text::host(url).is_none_or(|host| host.is_empty()) {
            return false;
        }
        let path = url_text::decoded_path(url);
        if !path.is_empty() && path != "/" {
            return true;
        }
        parsed.query().is_some_and(|query| !query.is_empty())
    }
}
