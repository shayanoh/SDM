//! The link grabber: URL extraction, verdict rules, package clustering and
//! the grabber's value types, link probing, and the grabber session.

mod fake_probe_origin;
mod file_signature;
mod grabber_session;
mod grabber_snapshot;
mod link_prober;
mod media_format_menu;
mod media_handoff;
mod media_metadata;
mod media_row;
mod package_clustering;
mod probe_transport;
mod probed_link;
mod release_tags;
mod reqwest_probe_transport;
mod url_extractor;
mod url_text;
mod verdict;
mod verdict_rules;

pub use fake_probe_origin::{FakeProbeOrigin, FakeProbeOriginBehavior};
pub use file_signature::FileSignature;
pub use grabber_session::{
    Clock, FfmpegAvailability, GrabberBudget, GrabberSession, GrabberSessionBuilder,
    QualityPreferencesProvider,
};
pub use grabber_snapshot::GrabberSnapshot;
pub use link_prober::LinkProber;
pub use media_format_menu::{MediaFormatMenu, MediaFormatOption};
pub use media_handoff::MediaHandoff;
pub use media_metadata::MediaMetadata;
pub use media_row::{MediaRow, MediaRowState};
pub use package_clustering::{ClusterableLink, PackageCandidate, PackageClustering};
pub use probe_transport::{ProbeError, ProbeMethod, ProbeRequest, ProbeResponse, ProbeTransport};
pub use probed_link::{ProbedLink, Stage};
pub use release_tags::ReleaseTags;
pub use reqwest_probe_transport::ReqwestProbeTransport;
pub use url_extractor::UrlExtractor;
pub use verdict::Verdict;
pub use verdict_rules::VerdictRules;
