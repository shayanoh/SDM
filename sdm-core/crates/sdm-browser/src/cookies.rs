//! The cookies the extension sends: the model, the site filter and the
//! Netscape `cookies.txt` writer yt-dlp reads.

use std::io::Write;
use std::path::Path;

use sdm_resolve::SiteRegistry;
use serde::Deserialize;

/// One cookie as `chrome.cookies` returns it; fields SDM does not use
/// (`hostOnly`, `session`, `storeId`, `sameSite`) are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeCookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    /// Seconds since the epoch; absent for a session cookie.
    #[serde(default)]
    pub expiration_date: Option<f64>,
}

/// The cookies that belong to a listed site and can be written to a
/// `cookies.txt` line without breaking it (no tab, CR or LF in a field).
/// Order is kept.
pub fn filter_cookies(cookies: Vec<ChromeCookie>) -> Vec<ChromeCookie> {
    cookies
        .into_iter()
        .filter(|cookie| SiteRegistry::cookie_domain_matches(&cookie.domain) && is_writable(cookie))
        .collect()
}

fn is_writable(cookie: &ChromeCookie) -> bool {
    [&cookie.domain, &cookie.name, &cookie.value, &cookie.path]
        .iter()
        .all(|field| !field.contains(['\t', '\r', '\n']))
}

/// `cookies.txt` in Netscape format, as yt-dlp's loader reads it: a header,
/// then one tab-separated row per cookie (domain, include-subdomains, path,
/// secure, expiry, name, value). HttpOnly cookies carry the `#HttpOnly_`
/// prefix; a session cookie has expiry `0`.
pub fn render_netscape(cookies: &[ChromeCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for cookie in cookies {
        let prefix = if cookie.http_only { "#HttpOnly_" } else { "" };
        let include_subdomains = bool_text(cookie.domain.starts_with('.'));
        let expiry = cookie.expiration_date.map_or(0, |seconds| seconds as u64);
        out.push_str(&format!(
            "{prefix}{}\t{include_subdomains}\t{}\t{}\t{expiry}\t{}\t{}\n",
            cookie.domain,
            cookie.path,
            bool_text(cookie.secure),
            cookie.name,
            cookie.value,
        ));
    }
    out
}

fn bool_text(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

/// Writes `contents` to `path` atomically: a temp file in the same folder
/// (owner-only on Unix), then a rename, so a reader never sees half a file.
/// Creates the folder when missing.
pub fn write_cookie_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let folder = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no folder")
    })?;
    std::fs::create_dir_all(folder)?;
    let temp = folder.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("cookies"),
        std::process::id()
    ));
    let result = (|| {
        let mut file = open_private(&temp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
