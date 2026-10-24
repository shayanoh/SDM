//! yt-dlp resolution and the subprocess seam.

mod binary_fetching;
mod binary_locator;
mod cookie_source;
mod format_selector;
pub mod lzfse;
mod managed_binaries;
mod managed_binaries_types;
mod platform_tools;
mod process_runner;
mod site_registry;
mod system_process_runner;
mod tool_install;
mod wholesale_progress_parser;
mod yt_dlp_json;
mod yt_dlp_resolver;
mod yt_dlp_wholesale_downloader;

#[cfg(feature = "testing")]
pub mod testing;

pub use binary_fetching::{BinaryFetchError, BinaryFetching, ReqwestBinaryFetcher};
pub use binary_locator::BinaryLocator;
pub use cookie_source::CookieSource;
pub use format_selector::FormatSelector;
pub use managed_binaries::{BinariesChangedFuture, ManagedBinaries, ManagedBinariesError};
pub use managed_binaries_types::{
    BinariesManifest, CheckOutcome, CheckReason, ManagedBinariesStatus, ToolsActivity, VendorAsset,
    YtDlpChannel, YtDlpVersion, yt_dlp_asset_name,
};
pub use platform_tools::{
    ArchiveKind, PlatformTool, ToolMember, UpdatePolicy, VersionSource, platform_tools,
};
pub use process_runner::{ProcessOutput, ProcessRunError, ProcessRunner};
pub use site_registry::{HostMatch, SitePattern, SiteRegistry};
pub use system_process_runner::SystemProcessRunner;
pub use tool_install::{ToolInstallError, install_tool};
pub use wholesale_progress_parser::WholesaleProgressParser;
pub use yt_dlp_json::{YtDlpDump, YtDlpEntry, YtDlpFormat, YtDlpParser};
pub use yt_dlp_resolver::{Classifier, YtDlpResolver};
pub use yt_dlp_wholesale_downloader::YtDlpWholesaleDownloader;
