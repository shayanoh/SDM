mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sdm_core::MediaContainer;
use sdm_engine::{FfmpegMuxer, MuxError, Muxer};
use sdm_resolve::testing::{FakeProcessRunner, fail};
use sdm_resolve::{BinaryLocator, SystemProcessRunner};
use support::{make_scratch_directory, within};
use tokio_util::sync::CancellationToken;

fn locator(has_ffmpeg: bool) -> Arc<BinaryLocator> {
    Arc::new(
        BinaryLocator::new()
            .with_search_paths(vec![PathBuf::from("/bin")])
            .with_is_executable(move |path| has_ffmpeg && path == Path::new("/bin/ffmpeg")),
    )
}

async fn mux_fixed_paths(muxer: &FfmpegMuxer) -> Result<(), MuxError> {
    muxer
        .mux(
            Path::new("/tmp/v"),
            Path::new("/tmp/a"),
            Path::new("/tmp/o.mp4"),
            &MediaContainer::Mp4,
            &CancellationToken::new(),
        )
        .await
}

#[test]
fn ffmpeg_arguments_copy_streams_and_set_faststart_for_mp4() {
    let args = FfmpegMuxer::arguments(
        Path::new("/tmp/v.f137.mp4"),
        Path::new("/tmp/a.f140.m4a"),
        Path::new("/tmp/out.mp4"),
        &MediaContainer::Mp4,
    );
    assert!(args.contains(&"-c".to_string()));
    assert!(args.contains(&"copy".to_string()));
    assert!(args.contains(&"+faststart".to_string()));
    assert_eq!(args.last().map(String::as_str), Some("/tmp/out.mp4"));
    // The whole vector, string for string.
    assert_eq!(
        args,
        [
            "-y",
            "-i",
            "/tmp/v.f137.mp4",
            "-i",
            "/tmp/a.f140.m4a",
            "-c",
            "copy",
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-movflags",
            "+faststart",
            "/tmp/out.mp4",
        ]
    );
}

#[test]
fn ffmpeg_arguments_omit_faststart_for_webm() {
    let args = FfmpegMuxer::arguments(
        Path::new("/tmp/v.webm"),
        Path::new("/tmp/a.webm"),
        Path::new("/tmp/out.webm"),
        &MediaContainer::Webm,
    );
    assert!(!args.contains(&"+faststart".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_without_ffmpeg_throws_missing() {
    let muxer = FfmpegMuxer::new(Arc::new(FakeProcessRunner::new()), locator(false));
    let result = within(mux_fixed_paths(&muxer)).await;
    assert_eq!(result, Err(MuxError::FfmpegMissing));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_surfaces_ffmpeg_stderr_on_failure() {
    let mut runner = FakeProcessRunner::new();
    runner.default_output = fail("Invalid data found when processing input\n", 1);
    let muxer = FfmpegMuxer::new(Arc::new(runner), locator(true));
    let result = within(mux_fixed_paths(&muxer)).await;
    assert_eq!(
        result,
        Err(MuxError::FfmpegFailed {
            stderr_tail: "Invalid data found when processing input\n".to_string()
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mux_succeeds_on_zero_exit() {
    let muxer = FfmpegMuxer::new(Arc::new(FakeProcessRunner::new()), locator(true));
    within(mux_fixed_paths(&muxer))
        .await
        .expect("a zero exit is a success");
}

/// Swift gates this on a real `ffmpeg` being installed. The Rust suite never
/// runs a real `ffmpeg`, so it is ignored; run it by hand with
/// `cargo test -p sdm-engine --test muxer_tests -- --ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "runs a real ffmpeg, which the suite never does"]
async fn real_ffmpeg_muxes_two_tiny_streams() {
    let dir = make_scratch_directory();
    let ffmpeg = BinaryLocator::new()
        .locate("ffmpeg")
        .expect("ffmpeg is installed");
    let video = dir.path().join("v.mp4");
    let audio = dir.path().join("a.m4a");
    let runner = SystemProcessRunner::new();
    let cancel = CancellationToken::new();
    let arguments = |parts: &[&str], output: &Path| -> Vec<String> {
        parts
            .iter()
            .map(|part| part.to_string())
            .chain([output.to_string_lossy().into_owned()])
            .collect()
    };
    use sdm_resolve::ProcessRunner;
    runner
        .run(
            &ffmpeg,
            &arguments(
                &[
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=duration=0.2:size=64x64:rate=10",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                ],
                &video,
            ),
            Duration::from_secs(30),
            &cancel,
        )
        .await
        .expect("make the video");
    runner
        .run(
            &ffmpeg,
            &arguments(
                &[
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=0.2",
                    "-c:a",
                    "aac",
                ],
                &audio,
            ),
            Duration::from_secs(30),
            &cancel,
        )
        .await
        .expect("make the audio");
    let output = dir.path().join("out.mp4");
    FfmpegMuxer::new(
        Arc::new(SystemProcessRunner::new()),
        Arc::new(BinaryLocator::new()),
    )
    .mux(&video, &audio, &output, &MediaContainer::Mp4, &cancel)
    .await
    .expect("mux");
    assert!(output.exists());
    assert!(std::fs::metadata(&output).expect("stat").len() > 0);
}
