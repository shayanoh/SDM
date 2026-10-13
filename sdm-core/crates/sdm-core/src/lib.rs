//! Domain models, `RangeSet`, themes. No I/O.

mod app_timing;
mod byte_range;
mod connection_allocator;
mod contrast_ratio;
mod download_item;
mod download_package;
mod file_component;
mod link_resolver;
mod media_format;
mod priority;
mod quality_preferences;
mod range_set;
mod resolved_media;
mod safe_path_component;
mod serde_support;
mod source_url;
mod theme;
mod theme_catalog;
mod wholesale_downloader;

pub use app_timing::AppTiming;
pub use byte_range::ByteRange;
pub use connection_allocator::{ConnectionAllocator, ConnectionBudget, ConnectionDemand};
pub use contrast_ratio::ContrastRatio;
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
pub use safe_path_component::{FALLBACK_NAME, safe_path_component, single_path_component};
pub use source_url::{SourceUrl, SourceUrlError};
pub use theme::Theme;
pub use theme_catalog::ThemeCatalog;
pub use wholesale_downloader::{
    WholesaleDownloader, WholesaleError, WholesalePhase, WholesaleProgress,
};
