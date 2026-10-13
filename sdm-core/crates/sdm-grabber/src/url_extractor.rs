use std::collections::HashSet;

use linkify::{LinkFinder, LinkKind};
use sdm_core::SourceUrl;

use crate::url_text;

/// Pulls `http`/`https` links out of arbitrary pasted or dropped text.
/// Spec §7.1: a link detector rather than a hand-written regex, so URLs
/// embedded in prose and wrapped by ordinary line breaks are found
/// correctly. Swift uses `NSDataDetector`; this port uses the `linkify`
/// crate, scheme-required (see the task report for where the two differ).
pub enum UrlExtractor {}

impl UrlExtractor {
    /// Distinct grabbable links in order of first appearance. Distinctness is
    /// by the URL string exactly as written.
    pub fn extract_links(text: &str) -> Vec<SourceUrl> {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url]);

        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();
        for link in finder.links(text) {
            let Ok(url) = SourceUrl::new(link.as_str()) else {
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
