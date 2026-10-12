use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sdm_resolve::{ProcessRunError, ProcessRunner, SystemProcessRunner};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captures_stdout_and_zero_exit() {
    let runner = SystemProcessRunner::new();
    let out = runner
        .run(
            Path::new("/bin/echo"),
            &["hello".to_string()],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(out.exit_code, 0);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captures_non_zero_exit() {
    let runner = SystemProcessRunner::new();
    let out = runner
        .run(
            Path::new("/bin/sh"),
            &["-c".to_string(), "exit 3".to_string()],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(out.exit_code, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn times_out_a_long_process() {
    let runner = SystemProcessRunner::new();
    let result = runner
        .run(
            Path::new("/bin/sleep"),
            &["10".to_string()],
            Duration::from_millis(200),
        )
        .await;
    assert_eq!(result, Err(ProcessRunError::TimedOut));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throws_launch_failed_for_missing_executable() {
    let runner = SystemProcessRunner::new();
    let result = runner
        .run(Path::new("/nonexistent/xyz"), &[], Duration::from_secs(1))
        .await;
    assert!(matches!(result, Err(ProcessRunError::LaunchFailed(_))));
}

/// Swift cancels the awaiting task; in Rust the caller cancels by dropping
/// the future. The child must receive SIGTERM (not SIGKILL): the script's
/// `TERM` trap proves it by leaving a marker file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_terminates_the_process() {
    let dir = std::env::temp_dir().join(format!("sdm-resolve-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("terminated");
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        "trap 'echo x > \"{}\"; kill $child; exit 0' TERM; sleep 10 & child=$!; echo ready; wait",
        marker.display()
    );

    let runner = SystemProcessRunner::new();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let arguments = ["-c".to_string(), script];
    let run = runner.run_streaming(
        Path::new("/bin/sh"),
        &arguments,
        Duration::from_secs(30),
        Arc::new(move |l| sink.lock().unwrap().push(l)),
    );
    // Cancel once the script has installed its trap.
    let cancelled = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::pin!(run);
        loop {
            tokio::select! {
                r = &mut run => return Some(r),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    if !lines.lock().unwrap().is_empty() { return None; }
                }
            }
        }
    })
    .await
    .expect("script never became ready");
    // `run` was dropped when the async block returned: that is the cancellation.
    assert!(cancelled.is_none(), "process ended before it was cancelled");

    let mut waited = 0;
    while !marker.exists() && waited < 500 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        waited += 1;
    }
    let existed = marker.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(existed, "child did not receive SIGTERM after cancellation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_streaming_forwards_lines_and_exit_code() {
    let runner = SystemProcessRunner::new();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let code = runner
        .run_streaming(
            Path::new("/bin/sh"),
            &[
                "-c".to_string(),
                "echo one; echo two; echo err 1>&2; exit 3".to_string(),
            ],
            Duration::from_secs(5),
            Arc::new(move |l| sink.lock().unwrap().push(l)),
        )
        .await
        .unwrap();
    assert_eq!(code, 3);
    let all = lines.lock().unwrap().clone();
    assert!(all.contains(&"one".to_string()));
    assert!(all.contains(&"two".to_string()));
    assert!(all.contains(&"err".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_streaming_times_out() {
    let runner = SystemProcessRunner::new();
    let result = runner
        .run_streaming(
            Path::new("/bin/sleep"),
            &["10".to_string()],
            Duration::from_millis(200),
            Arc::new(|_| {}),
        )
        .await;
    assert_eq!(result, Err(ProcessRunError::TimedOut));
}

#[test]
fn child_environment_prepends_tool_dirs_and_guarantees_home() {
    let env = SystemProcessRunner::child_environment(Path::new("/opt/custom/bin/yt-dlp"));
    let path = env.get("PATH").cloned().unwrap_or_default();
    assert!(path.starts_with("/opt/custom/bin:"));
    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/usr/local/bin"));
    assert!(path.contains("/usr/bin"));
    assert!(!env.get("HOME").cloned().unwrap_or_default().is_empty());
}
