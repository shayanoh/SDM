use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use url::Url;

/// Why a [`SourceUrl`] could not be built from a string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceUrlError {
    #[error("URL string is empty")]
    Empty,
    #[error("`{input}` is not an absolute URL: {reason}")]
    Unparseable {
        input: String,
        reason: url::ParseError,
    },
}

/// A URL that keeps exactly the string it was given.
///
/// Swift stores `URL.absoluteString`, which preserves things the `url` crate
/// normalizes (host case, a default port, an unescaped space, non-ASCII
/// path characters, a trailing dot, an empty query). Persisted URLs and the
/// de-duplication of grabbed links must survive byte for byte, so the
/// original string *is* the identity: it is what serializes, what compares
/// equal and what hashes. The parsed [`Url`] is only a view for APIs that
/// need one ([`SourceUrl::as_url`]). It does not imitate Foundation's
/// `URL(string:)` re-encoding quirks (for example `%20` becoming `%2520`
/// when the string also contains non-ASCII); the string is kept as given.
///
/// Construction ([`SourceUrl::new`]) requires a string the `url` crate can
/// parse and returns an error otherwise. Deserialization is deliberately
/// more lenient, to match Swift's `URL` `Decodable`, which throws only when
/// `URL(string:)` returns `nil`: a persisted string that Foundation accepts
/// but the `url` crate does not (a relative reference such as `a/b`) is kept
/// as-is with [`SourceUrl::as_url`] returning `None`, so one odd URL cannot
/// fail the whole state file. Only the empty string is rejected.
#[derive(Clone)]
pub struct SourceUrl {
    original: String,
    parsed: Option<Url>,
}

impl SourceUrl {
    /// Builds from a string, parsing it once.
    ///
    /// # Errors
    ///
    /// [`SourceUrlError::Empty`] for an empty string and
    /// [`SourceUrlError::Unparseable`] when the `url` crate rejects it.
    pub fn new(string: impl Into<String>) -> Result<Self, SourceUrlError> {
        let original = string.into();
        if original.is_empty() {
            return Err(SourceUrlError::Empty);
        }
        match Url::parse(&original) {
            Ok(parsed) => Ok(Self {
                original,
                parsed: Some(parsed),
            }),
            Err(reason) => Err(SourceUrlError::Unparseable {
                input: original,
                reason,
            }),
        }
    }

    /// Wraps an already-parsed URL; the string is its serialization.
    pub fn from_url(url: &Url) -> Self {
        Self {
            original: url.as_str().to_owned(),
            parsed: Some(url.clone()),
        }
    }

    /// The string exactly as it was given.
    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// The parsed form, or `None` for a deserialized string the `url` crate
    /// cannot parse (see the type docs).
    pub fn as_url(&self) -> Option<&Url> {
        self.parsed.as_ref()
    }
}

impl PartialEq for SourceUrl {
    fn eq(&self, other: &Self) -> bool {
        self.original == other.original
    }
}

impl Eq for SourceUrl {}

impl Hash for SourceUrl {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.original.hash(state);
    }
}

impl fmt::Debug for SourceUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SourceUrl").field(&self.original).finish()
    }
}

impl fmt::Display for SourceUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.original)
    }
}

impl From<Url> for SourceUrl {
    fn from(url: Url) -> Self {
        Self::from_url(&url)
    }
}

impl Serialize for SourceUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.original)
    }
}

impl<'de> Deserialize<'de> for SourceUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let original = String::deserialize(deserializer)?;
        if original.is_empty() {
            return Err(serde::de::Error::custom("Invalid URL string."));
        }
        let parsed = Url::parse(&original).ok();
        Ok(Self { original, parsed })
    }
}
