//! Turning what a person typed into a download the engine can take.

use percent_encoding::percent_decode_str;
use sdm_core::{SourceUrl, safe_path_component, windows_safe_name};

use crate::error::AppError;

/// Trims `input` and accepts only `http`/`https` URLs that have a host.
pub(crate) fn parse_download_url(input: &str) -> Result<SourceUrl, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("url", "the address is empty"));
    }
    let url = SourceUrl::new(trimmed)
        .map_err(|_| AppError::invalid("url", "that is not a valid web address"))?;
    let parsed = url
        .as_url()
        .ok_or_else(|| AppError::invalid("url", "that is not a valid web address"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::invalid(
            "url",
            "only http and https addresses can be downloaded",
        ));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(AppError::invalid("url", "the address has no host"));
    }
    Ok(url)
}

/// A file name for the download: the last non-empty path segment, percent
/// decoded, reduced to a single safe path component; `download` when none is
/// usable. Windows' reserved-name rule is applied on every OS so a download
/// added on a Mac can be moved to a Windows disk.
pub(crate) fn derive_filename(url: &SourceUrl) -> String {
    let segment = url
        .as_url()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        })
        .unwrap_or("");
    let decoded = percent_decode_str(segment).decode_utf8_lossy().into_owned();
    let component = safe_path_component(&decoded);
    // `windows_safe_name` is what Windows will apply anyway; running it here
    // too keeps names stable across machines (and handles `CON.txt`).
    let name = windows_safe_name(&component).unwrap_or_else(|| "download".to_owned());
    cap_bytes(name)
}

/// The longest file name macOS and Linux accept, in bytes (Windows counts
/// UTF-16 units instead, which `windows_safe_name` already limits).
const MAX_NAME_BYTES: usize = 255;

/// Shortens `name` to `MAX_NAME_BYTES` bytes on a character boundary, keeping
/// a short trailing extension, so multi-byte names do not fail at file creation.
fn cap_bytes(name: String) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name;
    }
    let extension_start = name
        .rfind('.')
        .filter(|&dot| dot > 0 && name.len() - dot <= 16)
        .unwrap_or(name.len());
    let (stem, extension) = name.split_at(extension_start);
    let mut end = MAX_NAME_BYTES.saturating_sub(extension.len());
    while end > 0 && !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{extension}", &stem[..end])
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_and_trims() {
        assert!(parse_download_url("  https://example.com/a.zip \n").is_ok());
        assert!(parse_download_url("http://example.com/a.zip").is_ok());
    }

    #[test]
    fn rejects_everything_else_with_a_clear_error() {
        for bad in [
            "",
            "   ",
            "not a url",
            "ftp://example.com/a",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "magnet:?xt=urn:btih:abc",
        ] {
            let error = parse_download_url(bad).expect_err(bad);
            assert!(
                matches!(error, AppError::InvalidArgument { .. }),
                "{bad}: {error:?}"
            );
        }
    }

    #[test]
    fn filenames_come_from_the_last_path_segment_decoded() {
        let cases = [
            (
                "https://example.com/files/report%20final.pdf",
                "report final.pdf",
            ),
            ("https://example.com/a/b/c.tar.gz?token=1#frag", "c.tar.gz"),
            (
                "https://example.com/%E6%97%A5%E6%9C%AC%E8%AA%9E.txt",
                "日本語.txt",
            ),
        ];
        for (url, expected) in cases {
            let url = parse_download_url(url).unwrap();
            assert_eq!(derive_filename(&url), expected);
        }
    }

    #[test]
    fn unusable_names_fall_back_and_never_escape() {
        for url in [
            "https://example.com/",
            "https://example.com",
            "https://example.com/?q=1",
            "https://example.com/%2e%2e",
            "https://example.com/a/..",
            "https://example.com/%00",
        ] {
            let name = derive_filename(&parse_download_url(url).unwrap());
            assert!(!name.is_empty(), "{url}");
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains('\0'),
                "{url}: {name:?}"
            );
            assert_ne!(name, ".");
            assert_ne!(name, "..");
        }
    }

    #[test]
    fn a_windows_reserved_name_is_made_safe_on_every_os() {
        let url = parse_download_url("https://example.com/CON.txt").unwrap();
        let name = derive_filename(&url);
        assert_eq!(name, "_CON.txt");
    }
    #[test]
    fn a_long_multibyte_name_fits_the_255_byte_file_name_limit() {
        // 120 three-byte characters: within 255 UTF-16 units, over 255 bytes.
        let name = "日".repeat(120);
        let url = parse_download_url(&format!("https://example.com/{name}.pdf")).unwrap();
        let derived = derive_filename(&url);
        assert!(derived.len() <= 255, "{} bytes", derived.len());
        assert!(derived.ends_with(".pdf"), "{derived}");
        assert!(derived.starts_with('日'));
    }
}
