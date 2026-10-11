//! Domain models, `RangeSet`, themes. No I/O.

uniffi::setup_scaffolding!();

mod app_timing;
mod byte_range;
mod connection_allocator;
mod download_item;
mod download_package;
mod file_component;
mod link_resolver;
mod media_format;
mod priority;
mod quality_preferences;
mod range_set;
mod resolved_media;
mod serde_support;
mod source_url;
mod wholesale_downloader;

pub use app_timing::AppTiming;
pub use byte_range::ByteRange;
pub use connection_allocator::{ConnectionAllocator, ConnectionBudget, ConnectionDemand};
pub use download_item::DownloadItem;
pub use download_package::DownloadPackage;
pub use file_component::{Assembly, ComponentError, ComponentOrigin, FileComponent};
pub use link_resolver::{LinkResolver, RefreshedFormat, ResolveError};
pub use media_format::{
    AudioCodec, MediaContainer, MediaDelivery, MediaFormat, MediaKind, VideoCodec,
};
pub use priority::{ItemState, Priority};
pub use quality_preferences::{FormatChoice, QualityPreferences};
pub use range_set::RangeSet;
pub use resolved_media::{ResolvedMedia, ResolvedTarget};
pub use source_url::{SourceUrl, SourceUrlError};
pub use wholesale_downloader::{
    WholesaleDownloader, WholesaleError, WholesalePhase, WholesaleProgress,
};

/// Temporary skeleton record proving a type defined here is exported from
/// `sdm-ffi` (replaced when the real domain types are ported).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoreVersion {
    pub version: String,
    pub crates: Vec<String>,
}
