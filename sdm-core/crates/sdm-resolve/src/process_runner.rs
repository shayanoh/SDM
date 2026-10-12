use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessRunError {
    #[error("launch failed: {0}")]
    LaunchFailed(String),
    #[error("timed out")]
    TimedOut,
    /// Twin of Swift throwing `CancellationError`: Rust has no ambient
    /// cancellation error, so cancelling the `cancel` token surfaces here.
    #[error("cancelled")]
    Cancelled,
}

/// `cancel` stands in for Swift's implicit task cancellation: cancelling it
/// terminates the child, and the call returns `Cancelled` only after the
/// child has exited (or the bounded drain has elapsed).
///
/// Injected everywhere a subprocess is run, so tests never spawn the real
/// `yt-dlp`/`ffmpeg`. Parent spec §4.5 / §10.2.
#[async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn run(
        &self,
        executable: &Path,
        arguments: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, ProcessRunError>;

    /// Runs the child and forwards each completed line of stdout **or**
    /// stderr as it arrives (for progress parsing). Returns the exit code;
    /// a non-zero code is not an error here. Honors `cancel` (Swift's task
    /// cancellation) and `timeout` by terminating the child
    /// (`ProcessRunError::Cancelled` / `ProcessRunError::TimedOut`). Parent spec
    /// `2026-09-03-multi-site-resolver-design.md` §6.6.
    ///
    /// Buffered fallback: run to completion, then replay the output
    /// line-by-line. Adequate for fakes and any runner that does not need
    /// true streaming.
    async fn run_streaming(
        &self,
        executable: &Path,
        arguments: &[String],
        timeout: Duration,
        on_line: Arc<dyn Fn(String) + Send + Sync>,
        cancel: &CancellationToken,
    ) -> Result<i32, ProcessRunError> {
        let out = self.run(executable, arguments, timeout, cancel).await?;
        for chunk in [&out.stdout, &out.stderr] {
            for line in split_lines_omitting_empty(&String::from_utf8_lossy(chunk)) {
                on_line(line);
            }
        }
        Ok(out.exit_code)
    }
}

/// Swift's `String.split(separator: "\n", omittingEmptySubsequences: true)`
/// compares `Character`s (grapheme clusters), and `"\r\n"` is a single
/// `Character` that is not `"\n"`, so a `\n` directly after a `\r` does not
/// split.
fn split_lines_omitting_empty(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut previous_was_cr = false;
    for ch in text.chars() {
        if ch == '\n' && !previous_was_cr {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
        previous_was_cr = ch == '\r';
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
