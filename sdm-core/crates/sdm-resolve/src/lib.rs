//! yt-dlp resolution and the subprocess seam.

mod process_runner;
mod system_process_runner;

#[cfg(feature = "testing")]
pub mod testing;

pub use process_runner::{ProcessOutput, ProcessRunError, ProcessRunner};
pub use system_process_runner::SystemProcessRunner;
