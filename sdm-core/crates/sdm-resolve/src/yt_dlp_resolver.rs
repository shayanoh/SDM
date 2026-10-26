use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use sdm_core::{
    LinkResolver, RefreshedFormat, ResolveError, ResolvedMedia, ResolvedTarget, SourceUrl,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    BinaryLocator, CookieLease, CookieProvider, CookieSource, ProcessOutput, ProcessRunError,
    ProcessRunner, SiteRegistry, YtDlpDump, YtDlpParser,
};

type CookieSourceFn = Arc<dyn Fn() -> CookieSource + Send + Sync>;
type MaxPlaylistVideosFn = Arc<dyn Fn() -> usize + Send + Sync>;
type ExtraArgumentsFn = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// yt-dlp as a metadata extractor only — never as a downloader. Parent
/// spec §4.4. All subprocess access goes through the injected `ProcessRunner`.
pub struct YtDlpResolver {
    runner: Arc<dyn ProcessRunner>,
    locator: Arc<BinaryLocator>,
    cookie_source: CookieSourceFn,
    /// Replaces `cookie_source` when set (the Tauri app's Chrome extension
    /// cookie file).
    cookie_provider: Option<CookieProvider>,
    max_playlist_videos: MaxPlaylistVideosFn,
    /// Tokens spliced into every yt-dlp invocation — the app supplies
    /// `--extractor-args youtube:jsruntime=quickjs` (QuickJS is off by
    /// default in yt-dlp) and `--ffmpeg-location <managed bin dir>`.
    extra_arguments: ExtraArgumentsFn,
    resolve_timeout: Duration,
    playlist_timeout: Duration,
}

impl YtDlpResolver {
    /// Timeouts take the Swift defaults (60 s and 120 s); see
    /// [`Self::with_timeouts`].
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        locator: Arc<BinaryLocator>,
        cookie_source: CookieSourceFn,
        max_playlist_videos: MaxPlaylistVideosFn,
        extra_arguments: ExtraArgumentsFn,
    ) -> Self {
        Self {
            runner,
            locator,
            cookie_source,
            cookie_provider: None,
            max_playlist_videos,
            extra_arguments,
            resolve_timeout: Duration::from_secs(60),
            playlist_timeout: Duration::from_secs(120),
        }
    }

    /// Swift's default closures: no cookies, 50 playlist videos, no extra
    /// arguments.
    pub fn with_defaults(runner: Arc<dyn ProcessRunner>, locator: Arc<BinaryLocator>) -> Self {
        Self::new(
            runner,
            locator,
            Arc::new(|| CookieSource::None),
            Arc::new(|| 50),
            Arc::new(Vec::new),
        )
    }

    /// Use `provider`'s arguments in place of the cookie source's; the lease
    /// is held for the whole yt-dlp run.
    pub fn with_cookie_provider(mut self, provider: CookieProvider) -> Self {
        self.cookie_provider = Some(provider);
        self
    }

    fn cookie_lease(&self) -> CookieLease {
        match &self.cookie_provider {
            Some(provider) => provider(),
            None => CookieLease::plain((self.cookie_source)().yt_dlp_arguments()),
        }
    }

    pub fn with_timeouts(mut self, resolve_timeout: Duration, playlist_timeout: Duration) -> Self {
        self.resolve_timeout = resolve_timeout;
        self.playlist_timeout = playlist_timeout;
        self
    }

    /// yt-dlp tokens this site needs on every invocation (usually none).
    fn per_site_arguments(url: &Url) -> Vec<String> {
        SiteRegistry::match_url(url)
            .map(|p| p.extra_args.clone())
            .unwrap_or_default()
    }

    /// A cheap heuristic that only chooses which yt-dlp invocation to *try
    /// first*. The parser then keys on yt-dlp's own `_type`, so a wrong
    /// guess self-corrects (spec §5.2).
    pub fn looks_like_playlist(&self, url: &Url) -> bool {
        if url.query_pairs().any(|(name, _)| name == "list") {
            return true;
        }
        if Self::is_channel_url(url) {
            return true;
        }
        let path = lowercased_path(url);
        if path.starts_with("/playlist") || path.contains("/sets/") {
            return true;
        }
        // Hints are matched anywhere in the path — a set / album / channel
        // segment (`/sets/`, `/album/`, `/r/`) usually sits mid-path.
        SiteRegistry::match_url(url)
            .map(|p| {
                p.playlist_path_hints
                    .iter()
                    .any(|hint| path.contains(hint.as_str()))
            })
            .unwrap_or(false)
    }

    fn is_channel_url(url: &Url) -> bool {
        let path = lowercased_path(url);
        path.starts_with("/@")
            || path.starts_with("/channel/")
            || path.starts_with("/c/")
            || path.starts_with("/user/")
            || path.ends_with("/videos")
    }

    // MARK: - Resolution

    async fn resolve_single(
        &self,
        source: &SourceUrl,
        url: &Url,
        cancel: &CancellationToken,
    ) -> Result<ResolvedTarget, ResolveError> {
        let yt_dlp = self.require_yt_dlp()?;
        let mut args = strings(&["-J", "--no-warnings", "--no-playlist"]);
        let lease = self.cookie_lease();
        args.extend(lease.arguments().iter().cloned());
        args.extend(Self::per_site_arguments(url));
        args.push(source.as_str().to_string());
        let out = self
            .run_yt_dlp(&yt_dlp, &args, self.resolve_timeout, cancel)
            .await
            .map_err(|error| with_cookie_hint(&lease, error))?;
        let dump = decode_dump(&out.stdout, "-J")?;
        // Some sites ignore `--no-playlist` and hand back a playlist anyway.
        if dump.type_.as_deref() == Some("playlist") {
            return self.playlist_target(&dump, url);
        }
        Ok(ResolvedTarget::Single(YtDlpParser::resolved_media(&dump)?))
    }

    async fn resolve_playlist(
        &self,
        source: &SourceUrl,
        url: &Url,
        cancel: &CancellationToken,
    ) -> Result<ResolvedTarget, ResolveError> {
        let yt_dlp = self.require_yt_dlp()?;
        let mut args = strings(&["-J", "--flat-playlist", "--no-warnings"]);
        let lease = self.cookie_lease();
        args.extend(lease.arguments().iter().cloned());
        args.extend(Self::per_site_arguments(url));
        args.push(source.as_str().to_string());
        let out = self
            .run_yt_dlp(&yt_dlp, &args, self.playlist_timeout, cancel)
            .await
            .map_err(|error| with_cookie_hint(&lease, error))?;
        let dump = decode_dump(&out.stdout, "--flat-playlist")?;
        // The guess was wrong and this is actually a single video.
        let no_entries = dump.entries.as_ref().is_none_or(|e| e.is_empty());
        if no_entries && dump.formats.as_ref().is_some_and(|f| !f.is_empty()) {
            return Ok(ResolvedTarget::Single(YtDlpParser::resolved_media(&dump)?));
        }
        self.playlist_target(&dump, url)
    }

    fn playlist_target(&self, dump: &YtDlpDump, url: &Url) -> Result<ResolvedTarget, ResolveError> {
        let all = YtDlpParser::flat_entries(dump, url);
        if all.is_empty() {
            return Err(ResolveError::Unavailable);
        }
        let cap = (self.max_playlist_videos)().clamp(10, 200);
        let take = cap.min(all.len());
        let kept = if Self::is_channel_url(url) {
            &all[..take]
        } else {
            &all[all.len() - take..]
        };
        let entries = kept
            .iter()
            .map(|(source_url, video_id, title)| {
                ResolvedMedia::new("", video_id.clone(), title.clone(), None, Vec::new())
                    .with_source_url(source_url.clone())
            })
            .collect();
        Ok(ResolvedTarget::Playlist {
            title: dump.title.clone().unwrap_or_else(|| "Playlist".to_string()),
            entries,
            total_available: all.len() as i64,
        })
    }

    // MARK: - Helpers

    fn require_yt_dlp(&self) -> Result<PathBuf, ResolveError> {
        self.locator
            .locate("yt-dlp")
            .ok_or(ResolveError::BinaryMissing)
    }

    async fn run_yt_dlp(
        &self,
        executable: &std::path::Path,
        arguments: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, ResolveError> {
        let mut full = arguments.to_vec();
        full.extend((self.extra_arguments)());
        let out = match self.runner.run(executable, &full, timeout, cancel).await {
            Ok(out) => out,
            Err(ProcessRunError::TimedOut) => return Err(ResolveError::Timeout),
            // Swift lets `CancellationError` escape here.
            Err(ProcessRunError::Cancelled) => return Err(ResolveError::Cancelled),
            // Swift lets the raw `ProcessRunError` escape; Rust's trait can
            // only return a `ResolveError`.
            Err(error @ ProcessRunError::LaunchFailed(_)) => {
                return Err(ResolveError::YtDlpFailed {
                    stderr_tail: error.to_string(),
                });
            }
        };
        if out.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Classifier::error_from_stderr(&stderr, out.exit_code));
        }
        Ok(out)
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Foundation's `URL.path` (percent-decoded), lowercased.
fn lowercased_path(url: &Url) -> String {
    percent_decode_str(url.path())
        .decode_utf8_lossy()
        .to_lowercase()
}

fn decode_dump(data: &[u8], context: &str) -> Result<YtDlpDump, ResolveError> {
    YtDlpDump::from_json(data).map_err(|_| ResolveError::YtDlpFailed {
        stderr_tail: format!("unparseable {context} output"),
    })
}

#[async_trait]
impl LinkResolver for YtDlpResolver {
    fn can_handle(&self, url: &SourceUrl) -> bool {
        url.as_url().is_some_and(|url| {
            matches!(url.scheme().to_lowercase().as_str(), "http" | "https")
                && SiteRegistry::match_url(url).is_some()
        })
    }

    async fn resolve(&self, source: &SourceUrl) -> Result<ResolvedTarget, ResolveError> {
        // No parsed form means no host or path to route on.
        let Some(url) = source.as_url() else {
            return Err(ResolveError::Unsupported);
        };
        // Nothing in the Swift app cancels a resolve (the grabber's
        // `cancelChecks()` only stops new work launching), so no token is
        // exposed; the runner gets one that never fires.
        let cancel = CancellationToken::new();
        if self.looks_like_playlist(url) {
            self.resolve_playlist(source, url, &cancel).await
        } else {
            self.resolve_single(source, url, &cancel).await
        }
    }

    async fn refresh(
        &self,
        source_url: &SourceUrl,
        format_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RefreshedFormat, ResolveError> {
        let Some(parsed) = source_url.as_url().filter(|_| self.can_handle(source_url)) else {
            return Err(ResolveError::Unsupported);
        };
        let yt_dlp = self.require_yt_dlp()?;
        // `--no-playlist` so a `watch?v=…&list=…` source still resolves the
        // one video; yt-dlp normalizes youtu.be / shorts / tracking params.
        let mut args = strings(&["-J", "--no-warnings", "--no-playlist"]);
        let lease = self.cookie_lease();
        args.extend(lease.arguments().iter().cloned());
        args.extend(Self::per_site_arguments(parsed));
        args.push(source_url.as_str().to_string());
        let out = self
            .run_yt_dlp(&yt_dlp, &args, self.resolve_timeout, cancel)
            .await
            .map_err(|error| with_cookie_hint(&lease, error))?;
        let dump = decode_dump(&out.stdout, "-J")?;
        let mapped = dump
            .formats
            .iter()
            .flatten()
            .find(|f| f.format_id.as_deref() == Some(format_id))
            .and_then(YtDlpParser::media_format)
            .ok_or(ResolveError::FormatGone)?;
        Ok(RefreshedFormat::new(
            mapped.url.clone(),
            mapped.filesize_effective(),
            format_id,
        ))
    }
}

/// A sign-in failure while the Chrome cookie file is missing is really "the
/// extension is not connected": say so instead of the generic wall message.
fn with_cookie_hint(lease: &CookieLease, error: ResolveError) -> ResolveError {
    if lease.is_chrome_unavailable() && error == ResolveError::AuthRequired {
        return ResolveError::YtDlpFailed {
            stderr_tail: "Sign-in is required, and Chrome cookies are unavailable: they come \
                          from the SDM Chrome extension, which is not connected. Open \
                          Settings → YouTube and choose Set up the extension."
                .to_owned(),
        };
    }
    error
}

/// Classifies yt-dlp's stderr into a `ResolveError` (`YtDlpResolver.Classifier`
/// in the Swift).
pub enum Classifier {}

impl Classifier {
    pub fn error_from_stderr(stderr: &str, _exit_code: i32) -> ResolveError {
        // Normalize typographic apostrophes (yt-dlp uses U+2019) so the
        // "you're not a bot" match actually fires.
        let lower = stderr.to_lowercase().replace('\u{2019}', "'");

        // YouTube served an anti-bot / consent wall with no session at
        // all — the fix is to provide cookies.
        if lower.contains("confirm you're not a bot")
            || lower.contains("sign in to confirm you're not a bot")
            || lower.contains("sign in to confirm your age")
            || (lower.contains("sign in") && lower.contains("cookies"))
        {
            return ResolveError::AuthRequired;
        }
        if lower.contains("private video") {
            return ResolveError::PrivateVideo;
        }
        if lower.contains("drm") {
            return ResolveError::DrmProtected;
        }
        // Generic sign-in walls across sites (Vimeo, Twitter, members-only).
        if lower.contains("requires you to log in")
            || lower.contains("log in to")
            || lower.contains("login required")
            || lower.contains("requires authentication")
            || lower.contains("this content isn't available")
        {
            return ResolveError::AuthRequired;
        }
        if lower.contains("video unavailable")
            || lower.contains("this video is not available")
            || lower.contains("has been removed")
            || lower.contains("available in your country")
            || lower.contains("available in your location")
            || lower.contains("geo-restricted")
            || lower.contains("geo restricted")
        {
            return ResolveError::Unavailable;
        }
        // Everything else: keep a generous slice of the *real* stderr so
        // it can reach the row's tooltip.
        ResolveError::YtDlpFailed {
            stderr_tail: suffix_chars(stderr.trim(), 2000),
        }
    }
}

/// Swift `String.suffix(n)` (by `Character`; Rust counts scalar values).
pub(crate) fn suffix_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}
