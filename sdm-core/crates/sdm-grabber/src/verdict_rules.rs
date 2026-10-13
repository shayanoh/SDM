use std::collections::HashSet;

use crate::probed_link::ProbedLink;
use crate::url_text;
use crate::verdict::Verdict;

const MEDIA_AND_ARCHIVE_EXTENSIONS: [&str; 12] = [
    "mp4", "mkv", "avi", "mov", "m4v", "zip", "rar", "7z", "gz", "tar", "iso", "pdf",
];

const SUSPICIOUS_PATH_TOKENS: [&str; 6] =
    ["login", "error", "expired", "404", "not-found", "denied"];

/// Table-driven pure function over a finished probe. Spec §7.3.
pub enum VerdictRules {}

impl VerdictRules {
    pub fn evaluate(link: &ProbedLink) -> Verdict {
        if link.transport_failed {
            return Verdict::CheckFailed;
        }
        let Some(status) = link.status_code else {
            return Verdict::CheckFailed;
        };
        if !(200..=299).contains(&status) {
            return Verdict::Offline;
        }

        let media_and_archive: HashSet<&str> = MEDIA_AND_ARCHIVE_EXTENSIONS.into_iter().collect();
        // Swift `split(separator: ".").last`: the last non-empty piece, so a
        // name with no dot at all yields the whole name.
        let ext = link
            .effective_filename()
            .split('.')
            .rfind(|piece| !piece.is_empty())
            .map(str::to_lowercase)
            .unwrap_or_default();
        let is_media_or_archive = media_and_archive.contains(ext.as_str());

        if is_media_or_archive
            && link
                .content_type
                .as_ref()
                .is_some_and(|t| t.to_lowercase() == "text/html")
        {
            return Verdict::Faulty {
                reason: format!("html, not {ext}"),
            };
        }

        if is_media_or_archive
            && let Some(length) = link.content_length
            && length < 1024
        {
            return Verdict::Faulty {
                reason: format!("too small to be a real {ext}"),
            };
        }

        if let Some(signature) = link.sniffed_signature
            && !ext.is_empty()
            && !signature.matches_extension(&ext)
        {
            return Verdict::Faulty {
                reason: format!("file signature does not match .{ext}"),
            };
        }

        let final_host = url_text::host(&link.final_url);
        if url_text::host(&link.original_url) != final_host {
            let path = url_text::decoded_path(&link.final_url).to_lowercase();
            if SUSPICIOUS_PATH_TOKENS
                .iter()
                .any(|token| path.contains(token))
            {
                return Verdict::Faulty {
                    reason: format!(
                        "redirected to {}",
                        final_host.as_deref().unwrap_or("unknown host")
                    ),
                };
            }
        }

        Verdict::Online
    }
}
