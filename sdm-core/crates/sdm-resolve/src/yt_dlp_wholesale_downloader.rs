use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdm_core::{ResolveError, SourceUrl, WholesaleDownloader, WholesaleError, WholesaleProgress};
use tokio_util::sync::CancellationToken;

use crate::yt_dlp_resolver::suffix_chars;
use crate::{
    BinaryLocator, Classifier, CookieSource, ProcessRunError, ProcessRunner, SiteRegistry,
    WholesaleProgressParser,
};

type CookieSourceFn = Arc<dyn Fn() -> CookieSource + Send + Sync>;
type ExtraArgumentsFn = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// Runs `yt-dlp` as a *downloader* for a stream with no single
/// `Range`-capable URL (HLS/DASH). The only place yt-dlp is not just a
/// metadata extractor. Resumable via `--continue` (the engine keeps the
/// partial output and re-runs). Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §6.6.
pub struct YtDlpWholesaleDownloader {
    runner: Arc<dyn ProcessRunner>,
    locator: Arc<BinaryLocator>,
    cookie_source: CookieSourceFn,
    extra_arguments: ExtraArgumentsFn,
    timeout: Duration,
}

impl YtDlpWholesaleDownloader {
    /// The timeout takes the Swift default (7200 s); see
    /// [`Self::with_timeout`].
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        locator: Arc<BinaryLocator>,
        cookie_source: CookieSourceFn,
        extra_arguments: ExtraArgumentsFn,
    ) -> Self {
        Self {
            runner,
            locator,
            cookie_source,
            extra_arguments,
            timeout: Duration::from_secs(7200),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl WholesaleDownloader for YtDlpWholesaleDownloader {
    async fn download(
        &self,
        page_url: &SourceUrl,
        format_selector: &str,
        destination: &Path,
        on_progress: Arc<dyn Fn(WholesaleProgress) + Send + Sync>,
        cancel: &CancellationToken,
    ) -> Result<(), WholesaleError> {
        let Some(yt_dlp) = self.locator.locate("yt-dlp") else {
            return Err(WholesaleError::BinaryMissing);
        };
        let ffmpeg_dir = yt_dlp
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let container = destination
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "mp4".to_string());

        let mut arguments: Vec<String> = [
            "-f",
            format_selector,
            "--no-playlist",
            "--newline",
            // Resume support: keep the `.part` / `.ytdl` scratch and
            // resume from the last completed fragment on the next run.
            // `--hls-prefer-native` forces yt-dlp's own fragment
            // downloader (the resumable one) over ffmpeg-as-downloader.
            // See `2026-09-03-wholesale-resume-design.md`.
            "--continue",
            "--hls-prefer-native",
            "--no-warnings",
            "--progress-template",
            concat!(
                "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s",
                "|%(progress.total_bytes_estimate)s|%(progress.fragment_index)s",
                "|%(progress.fragment_count)s"
            ),
            "--merge-output-format",
            &container,
            "--ffmpeg-location",
            &ffmpeg_dir,
            "-o",
            &destination.to_string_lossy(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(pattern) = page_url.as_url().and_then(SiteRegistry::match_url) {
            arguments.extend(pattern.extra_args.iter().cloned());
        }
        arguments.extend((self.cookie_source)().yt_dlp_arguments());
        arguments.extend((self.extra_arguments)());
        arguments.push(page_url.as_str().to_string());

        let tail = Arc::new(StderrTail::default());
        let line_tail = tail.clone();
        let on_line: Arc<dyn Fn(String) + Send + Sync> =
            Arc::new(move |line| match WholesaleProgressParser::parse(&line) {
                Some(progress) => on_progress(progress),
                None => line_tail.consider(&line),
            });

        let exit_code = match self
            .runner
            .run_streaming(&yt_dlp, &arguments, self.timeout, on_line, cancel)
            .await
        {
            Ok(code) => code,
            Err(ProcessRunError::Cancelled) => return Err(WholesaleError::Cancelled),
            Err(ProcessRunError::TimedOut) => {
                return Err(WholesaleError::Failed {
                    stderr_tail: "yt-dlp timed out".to_string(),
                });
            }
            // Swift lets the raw `ProcessRunError` escape; Rust's trait can
            // only return a `WholesaleError`.
            Err(error @ ProcessRunError::LaunchFailed(_)) => {
                return Err(WholesaleError::Failed {
                    stderr_tail: error.to_string(),
                });
            }
        };

        if exit_code != 0 {
            let stderr = tail.value();
            return Err(match Classifier::error_from_stderr(&stderr, exit_code) {
                ResolveError::AuthRequired => WholesaleError::AuthRequired,
                ResolveError::PrivateVideo
                | ResolveError::Unavailable
                | ResolveError::DrmProtected => WholesaleError::Unavailable,
                _ => WholesaleError::Failed {
                    stderr_tail: if stderr.is_empty() {
                        format!("yt-dlp exited {exit_code}")
                    } else {
                        stderr
                    },
                },
            });
        }

        if !destination.exists() {
            return Err(WholesaleError::Failed {
                stderr_tail: "yt-dlp produced no output file".to_string(),
            });
        }
        Ok(())
    }
}

/// Keeps the last ~2 KB of non-progress output for error reporting.
#[derive(Default)]
struct StderrTail {
    lines: Mutex<Vec<String>>,
}

impl StderrTail {
    fn consider(&self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut lines = self.lines.lock().unwrap();
        lines.push(trimmed.to_string());
        if lines.len() > 60 {
            let excess = lines.len() - 60;
            lines.drain(..excess);
        }
    }

    fn value(&self) -> String {
        suffix_chars(&self.lines.lock().unwrap().join("\n"), 2000)
    }
}
