//! The clipboard watcher's decision logic: what in a stream of clipboard
//! reads is worth ingesting. A port of the SwiftUI `ClipboardWatcher`, minus
//! the pasteboard itself (the shell reads the clipboard and calls in).
//!
//! Only a hash of the last text is kept. Clipboard text that is not a link is
//! never stored or logged.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Default)]
pub struct ClipboardWatcher {
    last: Option<u64>,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// `text` when it differs from the previous read; `None` for a repeat, an
    /// empty or non-text clipboard (which keeps the previous read, so the
    /// same text coming back after a gap is still the same copy).
    pub fn changed<'a>(&mut self, text: Option<&'a str>) -> Option<&'a str> {
        let text = text?;
        let now = fingerprint(text);
        if self.last == Some(now) {
            return None;
        }
        self.last = Some(now);
        Some(text)
    }

    /// Marks `text`, which SDM just put on the clipboard itself, as already
    /// seen, so it is not grabbed back.
    pub fn ignore_own_write(&mut self, text: &str) {
        self.last = Some(fingerprint(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_text_is_returned_once() {
        let mut watcher = ClipboardWatcher::new();
        assert_eq!(
            watcher.changed(Some("https://a.example/x")),
            Some("https://a.example/x")
        );
        assert_eq!(watcher.changed(Some("https://a.example/x")), None);
        assert_eq!(
            watcher.changed(Some("https://b.example/y")),
            Some("https://b.example/y")
        );
    }

    #[test]
    fn an_empty_or_non_text_clipboard_returns_nothing_and_keeps_the_last_text() {
        let mut watcher = ClipboardWatcher::new();
        assert_eq!(watcher.changed(None), None);
        assert_eq!(
            watcher.changed(Some("https://a.example/x")),
            Some("https://a.example/x")
        );
        assert_eq!(watcher.changed(None), None);
        // The same text coming back after a gap is still the same copy.
        assert_eq!(watcher.changed(Some("https://a.example/x")), None);
    }

    #[test]
    fn our_own_write_is_ignored_once_and_a_later_real_copy_of_it_is_not() {
        let mut watcher = ClipboardWatcher::new();
        watcher.ignore_own_write("https://a.example/x");
        assert_eq!(watcher.changed(Some("https://a.example/x")), None);
        assert_eq!(
            watcher.changed(Some("something else")),
            Some("something else")
        );
        assert_eq!(
            watcher.changed(Some("https://a.example/x")),
            Some("https://a.example/x")
        );
    }
}
