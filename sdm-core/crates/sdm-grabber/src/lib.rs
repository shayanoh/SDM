//! The link grabber: URL extraction, verdict rules, package clustering and
//! the grabber's value types. Task 20 holds the pure functions; probing and
//! the session come in later tasks.

mod file_signature;
mod grabber_snapshot;
mod media_format_menu;
mod media_handoff;
mod media_metadata;
mod media_row;
mod package_clustering;
mod probed_link;
mod release_tags;
mod url_extractor;
mod url_text;
mod verdict;
mod verdict_rules;

pub use file_signature::FileSignature;
pub use grabber_snapshot::GrabberSnapshot;
pub use media_format_menu::{MediaFormatMenu, MediaFormatOption};
pub use media_handoff::MediaHandoff;
pub use media_metadata::MediaMetadata;
pub use media_row::{MediaRow, MediaRowState};
pub use package_clustering::{ClusterableLink, PackageCandidate, PackageClustering};
pub use probed_link::{ProbedLink, Stage};
pub use release_tags::ReleaseTags;
pub use url_extractor::UrlExtractor;
pub use verdict::Verdict;
pub use verdict_rules::VerdictRules;
