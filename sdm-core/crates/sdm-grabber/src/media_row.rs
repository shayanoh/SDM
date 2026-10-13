use std::sync::LazyLock;

use regex::Regex;
use sdm_core::{FormatChoice, MediaContainer, ResolvedMedia, SourceUrl};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::url_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaRowState {
    Resolving,
    Resolved,
    Unselected,
    Unsupported,
    NeedsYtDlp,
    NeedsFfmpeg,
    Failed(String),
}

/// One YouTube (or other extractor-backed) URL in the linkgrabber — the
/// media counterpart of `ProbedLink`. Parent spec §6.2.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRow {
    pub id: Uuid,
    pub source_url: SourceUrl,
    pub title: String,
    pub state: MediaRowState,
    pub media: Option<ResolvedMedia>,
    pub choice: Option<FormatChoice>,
    pub playlist_group: Option<Uuid>,
    pub is_duplicate: bool,
}

impl MediaRow {
    /// A `Resolving` row with a fresh id, titled after the URL's last path
    /// component (`video` when it has none). The other Swift initializer
    /// arguments are the `with_*` builders (or plain field assignment).
    pub fn new(source_url: SourceUrl) -> Self {
        let last = url_text::last_path_component(&source_url);
        Self {
            id: Uuid::new_v4(),
            title: if last.is_empty() {
                "video".into()
            } else {
                last
            },
            source_url,
            state: MediaRowState::Resolving,
            media: None,
            choice: None,
            playlist_group: None,
            is_duplicate: false,
        }
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_state(mut self, state: MediaRowState) -> Self {
        self.state = state;
        self
    }

    pub fn with_media(mut self, media: ResolvedMedia) -> Self {
        self.media = Some(media);
        self
    }

    pub fn with_choice(mut self, choice: FormatChoice) -> Self {
        self.choice = Some(choice);
        self
    }

    pub fn with_playlist_group(mut self, playlist_group: Uuid) -> Self {
        self.playlist_group = Some(playlist_group);
        self
    }

    pub fn with_duplicate(mut self, is_duplicate: bool) -> Self {
        self.is_duplicate = is_duplicate;
        self
    }

    /// `"<sanitized title> [<videoID>].<container>"` once resolved with a
    /// chosen format; the URL-derived title before that. Used for clustering
    /// and as the eventual `DownloadItem.outputFilename`.
    pub fn display_filename(&self) -> String {
        let (Some(media), Some(choice)) = (&self.media, &self.choice) else {
            return self.title.clone();
        };
        let container: &MediaContainer = &choice.output_container;
        format!(
            "{} [{}].{}",
            Self::sanitize(&media.title),
            media.video_id,
            container.file_extension()
        )
    }

    pub fn combined_bytes(&self) -> Option<i64> {
        self.choice.as_ref().and_then(|c| c.estimated_bytes)
    }

    pub(crate) fn sanitize(raw: &str) -> String {
        // Control characters (Unicode `Cc`, which includes tab and newline,
        // plus `Cf`, the invisible format characters such as the zero-width
        // non-joiner) are dropped outright, not turned into spaces: that is
        // what `CharacterSet.controlCharacters` holds.
        let slashless: String = raw.chars().filter(|c| *c != '/').collect();
        let kept = CONTROL_CHARACTERS.replace_all(&slashless, "");
        let collapsed = WHITESPACE_RUNS.replace_all(&kept, " ");
        let collapsed = collapsed.trim_matches(|c: char| c == '\t' || is_space_separator(c));
        if collapsed.is_empty() {
            "video".into()
        } else {
            collapsed.graphemes(true).take(180).collect()
        }
    }
}

/// `CharacterSet.controlCharacters`: Unicode general categories `Cc` and `Cf`.
static CONTROL_CHARACTERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Cf}]").expect("valid regex"));

static WHITESPACE_RUNS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));

/// Unicode general category `Zs` (what `CharacterSet.whitespaces` holds
/// besides tab).
fn is_space_separator(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

#[cfg(test)]
mod tests {
    use super::MediaRow;

    /// Swift's `CharacterSet.controlCharacters` is Unicode `Cc` plus `Cf`, so
    /// it strips the zero-width non-joiner that Persian titles contain, and
    /// the other invisible format characters.
    #[test]
    fn sanitize_strips_format_characters_like_swift() {
        // "mikhaham" with a ZWNJ between the two words.
        assert_eq!(MediaRow::sanitize("می\u{200C}خواهم"), "میخواهم");
        assert_eq!(
            MediaRow::sanitize("\u{FEFF}a\u{200B}b\u{200E}c\u{200F}d\u{00AD}e"),
            "abcde"
        );
        assert_eq!(MediaRow::sanitize("\u{200C}\u{200D}"), "video");
        // Still dropped: Cc (tab, newline) and the path separator.
        assert_eq!(MediaRow::sanitize("a\tb\nc/d"), "abcd");
    }
}
