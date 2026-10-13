use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdm_core::{SourceUrl, WholesaleDownloader, WholesaleError, WholesalePhase, WholesaleProgress};
use sdm_resolve::{
    BinaryLocator, CookieSource, ProcessOutput, ProcessRunError, ProcessRunner,
    YtDlpWholesaleDownloader,
};
use tokio_util::sync::CancellationToken;

fn u(s: &str) -> SourceUrl {
    SourceUrl::new(s).unwrap()
}

fn wholesale_locator(has_yt_dlp: bool) -> Arc<BinaryLocator> {
    Arc::new(
        BinaryLocator::new()
            .with_search_paths(vec!["/bin".into()])
            .with_is_executable(move |p| has_yt_dlp && p.to_str() == Some("/bin/yt-dlp")),
    )
}

fn downloader(runner: Arc<FakeStreamingRunner>, has_yt_dlp: bool) -> YtDlpWholesaleDownloader {
    YtDlpWholesaleDownloader::new(
        runner,
        wholesale_locator(has_yt_dlp),
        Arc::new(|| CookieSource::None),
        Arc::new(Vec::new),
    )
}

/// Replays a scripted line sequence, then exits with `exit_code`. Optionally
/// creates the destination file (parsed out of `-o <path>`).
struct FakeStreamingRunner {
    lines: Vec<String>,
    exit_code: i32,
    creates_output: bool,
    last_arguments: Mutex<Vec<String>>,
}

impl FakeStreamingRunner {
    fn new(lines: &[&str], exit_code: i32, creates_output: bool) -> Arc<Self> {
        Arc::new(Self {
            lines: lines.iter().map(|s| s.to_string()).collect(),
            exit_code,
            creates_output,
            last_arguments: Mutex::new(Vec::new()),
        })
    }

    fn last_arguments(&self) -> Vec<String> {
        self.last_arguments.lock().unwrap().clone()
    }
}

#[async_trait]
impl ProcessRunner for FakeStreamingRunner {
    async fn run(
        &self,
        _executable: &Path,
        _arguments: &[String],
        _timeout: Duration,
        _cancel: &CancellationToken,
    ) -> Result<ProcessOutput, ProcessRunError> {
        Ok(ProcessOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: self.exit_code,
        })
    }

    async fn run_streaming(
        &self,
        _executable: &Path,
        arguments: &[String],
        _timeout: Duration,
        on_line: Arc<dyn Fn(String) + Send + Sync>,
        _cancel: &CancellationToken,
    ) -> Result<i32, ProcessRunError> {
        *self.last_arguments.lock().unwrap() = arguments.to_vec();
        for line in &self.lines {
            on_line(line.clone());
        }
        if self.creates_output
            && let Some(o_index) = arguments.iter().position(|a| a == "-o")
            && o_index + 1 < arguments.len()
        {
            std::fs::write(&arguments[o_index + 1], b"x").unwrap();
        }
        Ok(self.exit_code)
    }
}

/// A destination in the per-process temp dir, removed on drop.
struct TempDest(PathBuf);

impl TempDest {
    fn new() -> Self {
        TempDest(std::env::temp_dir().join(format!("{}.mp4", uuid::Uuid::new_v4())))
    }
}

impl Drop for TempDest {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

type ProgressSink = Arc<dyn Fn(WholesaleProgress) + Send + Sync>;

fn collector() -> (Arc<Mutex<Vec<WholesaleProgress>>>, ProgressSink) {
    let store = Arc::new(Mutex::new(Vec::new()));
    let sink = store.clone();
    (store, Arc::new(move |p| sink.lock().unwrap().push(p)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emits_progress_and_succeeds() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(
        &[
            "sdm:downloading|500|1000|NA| 50.0%",
            "sdm:downloading|1000|1000|NA| 100.0%",
            "[Merger] Merging formats into \"o.mp4\"",
        ],
        0,
        true,
    );
    let d = downloader(runner.clone(), true);
    let (store, on_progress) = collector();
    d.download(
        &u("https://x.com/1"),
        "bv*+ba/b",
        &dest.0,
        on_progress,
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    let all = store.lock().unwrap().clone();
    assert_eq!(
        all.last().map(|p| p.phase),
        Some(WholesalePhase::PostProcessing)
    );
    assert!(all.iter().any(|p| p.downloaded_bytes == Some(1000)));
    let args = runner.last_arguments();
    assert!(args.contains(&"-f".to_string()));
    assert!(args.contains(&"bv*+ba/b".to_string()));
    assert!(args.contains(&"--newline".to_string()));
    assert!(args.contains(&"https://x.com/1".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_fragment_resume_and_native_hls() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(&["sdm:downloading|1|1|NA|1|1"], 0, true);
    let d = downloader(runner.clone(), true);
    d.download(
        &u("https://x.com/1"),
        "b",
        &dest.0,
        Arc::new(|_| {}),
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    let args = runner.last_arguments();
    assert!(args.contains(&"--continue".to_string()));
    assert!(args.contains(&"--hls-prefer-native".to_string()));
    assert!(!args.contains(&"--no-continue".to_string()));
    assert!(!args.contains(&"--no-part".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_zero_exit_throws_failed_with_tail() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(&["ERROR: [x] boom happened"], 1, false);
    let d = downloader(runner, true);
    let result = d
        .download(
            &u("https://x.com/1"),
            "b",
            &dest.0,
            Arc::new(|_| {}),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result,
        Err(WholesaleError::Failed {
            stderr_tail: "ERROR: [x] boom happened".to_string()
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drm_exit_throws_unavailable() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(&["ERROR: This video is DRM protected"], 1, false);
    let d = downloader(runner, true);
    let result = d
        .download(
            &u("https://x.com/1"),
            "b",
            &dest.0,
            Arc::new(|_| {}),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(result, Err(WholesaleError::Unavailable));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_binary_throws() {
    let d = downloader(FakeStreamingRunner::new(&[], 0, true), false);
    let result = d
        .download(
            &u("https://x.com/1"),
            "b",
            Path::new("/tmp/o.mp4"),
            Arc::new(|_| {}),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(result, Err(WholesaleError::BinaryMissing));
}

/// Not in the Swift suite: the runner's cancellation maps to
/// `WholesaleError::Cancelled`, a timeout to `Failed("yt-dlp timed out")`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runner_cancellation_and_timeout_map_to_wholesale_errors() {
    struct Failing(ProcessRunError);

    #[async_trait]
    impl ProcessRunner for Failing {
        async fn run(
            &self,
            _: &Path,
            _: &[String],
            _: Duration,
            _: &CancellationToken,
        ) -> Result<ProcessOutput, ProcessRunError> {
            Err(self.0.clone())
        }
    }

    for (error, expected) in [
        (ProcessRunError::Cancelled, WholesaleError::Cancelled),
        (
            ProcessRunError::TimedOut,
            WholesaleError::Failed {
                stderr_tail: "yt-dlp timed out".to_string(),
            },
        ),
    ] {
        let d = YtDlpWholesaleDownloader::new(
            Arc::new(Failing(error)),
            wholesale_locator(true),
            Arc::new(|| CookieSource::None),
            Arc::new(Vec::new),
        );
        let result = d
            .download(
                &u("https://x.com/1"),
                "b",
                Path::new("/tmp/o.mp4"),
                Arc::new(|_| {}),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(result, Err(expected));
    }
}

/// The complete yt-dlp argument vector, in order, with the page URL passed
/// as its original string (upper-case host kept, as Swift's `absoluteString`
/// does), the cookie arguments and the injected extra arguments in place.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn passes_the_complete_argument_vector_with_the_original_url() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(&["sdm:downloading|1|1|NA|1|1"], 0, true);
    let d = YtDlpWholesaleDownloader::new(
        runner.clone(),
        wholesale_locator(true),
        Arc::new(|| CookieSource::Safari),
        Arc::new(|| vec!["--sponsorblock-mark".to_string(), "none".to_string()]),
    );
    d.download(
        &u("https://WWW.Example.COM/Watch?v=AbC"),
        "bv*+ba/b",
        &dest.0,
        Arc::new(|_| {}),
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    let expected: Vec<String> = [
        "-f",
        "bv*+ba/b",
        "--no-playlist",
        "--newline",
        "--continue",
        "--hls-prefer-native",
        "--no-warnings",
        "--progress-template",
        "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.fragment_index)s|%(progress.fragment_count)s",
        "--merge-output-format",
        "mp4",
        "--ffmpeg-location",
        "/bin",
        "-o",
        dest.0.to_str().unwrap(),
        "--cookies-from-browser",
        "safari",
        "--sponsorblock-mark",
        "none",
        "https://WWW.Example.COM/Watch?v=AbC",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(runner.last_arguments(), expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_url_with_no_parsed_form_still_downloads_with_its_string() {
    let dest = TempDest::new();
    let runner = FakeStreamingRunner::new(&[], 0, true);
    let relative: SourceUrl = serde_json::from_str("\"a/b\"").unwrap();
    downloader(runner.clone(), true)
        .download(
            &relative,
            "b",
            &dest.0,
            Arc::new(|_| {}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        runner.last_arguments().last().map(String::as_str),
        Some("a/b")
    );
}
