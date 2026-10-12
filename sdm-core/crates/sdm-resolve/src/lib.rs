//! yt-dlp resolution and the subprocess seam.

mod binary_locator;
mod cookie_source;
mod format_selector;
mod process_runner;
mod site_registry;
mod system_process_runner;
mod wholesale_progress_parser;
mod yt_dlp_json;
mod yt_dlp_resolver;
mod yt_dlp_wholesale_downloader;

#[cfg(feature = "testing")]
pub mod testing;

pub use binary_locator::BinaryLocator;
pub use cookie_source::CookieSource;
pub use format_selector::FormatSelector;
pub use process_runner::{ProcessOutput, ProcessRunError, ProcessRunner};
pub use site_registry::{HostMatch, SitePattern, SiteRegistry};
pub use system_process_runner::SystemProcessRunner;
pub use wholesale_progress_parser::WholesaleProgressParser;
pub use yt_dlp_json::{YtDlpDump, YtDlpEntry, YtDlpFormat, YtDlpParser};
pub use yt_dlp_resolver::{Classifier, YtDlpResolver};
pub use yt_dlp_wholesale_downloader::YtDlpWholesaleDownloader;
