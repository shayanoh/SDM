use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sdm_resolve::{ProcessRunError, ProcessRunner, SystemProcessRunner};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captures_stdout_and_zero_exit() {
    let runner = SystemProcessRunner::new();
    let out = runner
        .run(
            Path::new("/bin/echo"),
            &["hello".to_string()],
            Duration::from_secs(5),
            &CancellationToken::new(),
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
            &CancellationToken::new(),
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
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(result, Err(ProcessRunError::TimedOut));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throws_launch_failed_for_missing_executable() {
    let runner = SystemProcessRunner::new();
    let result = runner
        .run(
            Path::new("/nonexistent/xyz"),
            &[],
            Duration::from_secs(1),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(ProcessRunError::LaunchFailed(_))));
}

/// What the test scripts do after printing `ready`: idle in short foreground
/// sleeps. The scripts used to end in `sleep 10 & ...; wait`, but the shell
/// (macOS `/bin/sh`, bash 3.2) can lose a SIGTERM that arrives between its
/// `echo ready` and the moment it blocks in `wait`: the trap then never runs
/// (or runs only when the sleep ends, or the shell crashes). The tests send
/// the signal the instant they see `ready`, so under CPU load they hit that
/// window. A loop of short sleeps bounds the trap's latency to one sleep.
const IDLE_LOOP: &str = "while :; do sleep 0.05; done";

fn marker_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sdm-resolve-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Cancelling the token terminates the child (SIGTERM), waits for it to
/// exit, and returns `Cancelled`. The script's `TERM` trap takes ~0.3 s to
/// write its marker, so the marker existing on return proves the call
/// waited for the child instead of returning while it was still alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_terminates_the_process() {
    let dir = marker_dir("cancel");
    let marker = dir.join("terminated");
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        "trap 'sleep 0.3; echo x > \"{}\"; exit 0' TERM; echo ready; {}",
        marker.display(),
        IDLE_LOOP
    );

    let runner = SystemProcessRunner::new();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let arguments = ["-c".to_string(), script];
    let started = Instant::now();
    let result = runner
        .run_streaming(
            Path::new("/bin/sh"),
            &arguments,
            Duration::from_secs(30),
            Arc::new(move |line| {
                if line == "ready" {
                    trigger.cancel();
                }
            }),
            &cancel,
        )
        .await;
    let elapsed = started.elapsed();
    let existed = marker.exists();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(result, Err(ProcessRunError::Cancelled));
    assert!(existed, "returned before the child had exited");
    assert!(elapsed < Duration::from_secs(3), "drain took {elapsed:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_applies_to_run_too() {
    let runner = SystemProcessRunner::new();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let result = runner
        .run(
            Path::new("/bin/sleep"),
            &["10".to_string()],
            Duration::from_secs(30),
            &cancel,
        )
        .await;
    canceller.await.unwrap();
    assert_eq!(result, Err(ProcessRunError::Cancelled));
}

/// Swift launches the child before it looks at cancellation, then
/// terminates it; the call still ends in a cancellation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn already_cancelled_token_returns_cancelled() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = SystemProcessRunner::new()
        .run(
            Path::new("/bin/sleep"),
            &["10".to_string()],
            Duration::from_secs(30),
            &cancel,
        )
        .await;
    assert_eq!(result, Err(ProcessRunError::Cancelled));
}

/// Backstop: a future dropped without cancelling the token still gets its
/// child SIGTERMed (the trap leaves a marker).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_future_still_terminates_the_child() {
    let dir = marker_dir("drop");
    let marker = dir.join("terminated");
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        "trap 'echo x > \"{}\"; exit 0' TERM; echo ready; {}",
        marker.display(),
        IDLE_LOOP
    );
    let runner = SystemProcessRunner::new();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let ready_tx = Mutex::new(Some(ready_tx));
    let arguments = ["-c".to_string(), script];
    let cancel = CancellationToken::new();
    {
        let run = runner.run_streaming(
            Path::new("/bin/sh"),
            &arguments,
            Duration::from_secs(30),
            Arc::new(move |_| {
                if let Some(tx) = ready_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            }),
            &cancel,
        );
        tokio::pin!(run);
        tokio::select! {
            _ = &mut run => panic!("process ended before it was dropped"),
            _ = ready_rx => {}
        }
    } // `run` dropped here
    // Wait for the observable event (the trap's marker) with a generous
    // bound: on a loaded machine the SIGTERM can take seconds to be
    // delivered and handled, and a pass returns as soon as the marker
    // appears, so the bound only matters when the behaviour is broken.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let existed = marker.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(existed, "child did not receive SIGTERM after the drop");
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
            &CancellationToken::new(),
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
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(result, Err(ProcessRunError::TimedOut));
}

fn os(s: &str) -> std::ffi::OsString {
    std::ffi::OsString::from(s)
}

fn path_of(env: &std::collections::HashMap<std::ffi::OsString, std::ffi::OsString>) -> String {
    env.get(std::ffi::OsStr::new("PATH"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn child_environment_prepends_tool_dirs_and_guarantees_home() {
    let env = SystemProcessRunner::child_environment(Path::new("/opt/custom/bin/yt-dlp"));
    let path = path_of(&env);
    assert!(path.starts_with("/opt/custom/bin:"));
    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/usr/local/bin"));
    assert!(path.contains("/usr/bin"));
    assert!(
        !env.get(std::ffi::OsStr::new("HOME"))
            .cloned()
            .unwrap_or_default()
            .is_empty()
    );
}

#[test]
fn child_environment_keeps_non_utf8_variables_untouched() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bad_value = std::ffi::OsString::from_vec(vec![b'v', 0xFF, 0xFE, b'x']);
    let bad_name = std::ffi::OsString::from_vec(vec![b'N', 0xC3, 0x28]);
    let bad_path_entry = std::ffi::OsString::from_vec(b"/weird/\xFF/bin".to_vec());
    let mut path = std::ffi::OsString::from("/usr/bin:");
    path.push(&bad_path_entry);
    let inherited = vec![
        (os("KEEP"), bad_value.clone()),
        (bad_name.clone(), os("value")),
        (os("PATH"), path),
    ];
    let env = SystemProcessRunner::child_environment_from(
        Path::new("/opt/custom/bin/yt-dlp"),
        inherited,
        std::path::PathBuf::from("/Users/someone"),
    );
    assert_eq!(env.get(std::ffi::OsStr::new("KEEP")), Some(&bad_value));
    assert_eq!(env.get(&bad_name), Some(&os("value")));
    // The non-UTF-8 PATH entry survives byte for byte, and `/usr/bin` is not
    // repeated.
    let merged = env.get(std::ffi::OsStr::new("PATH")).unwrap().as_bytes();
    let entries: Vec<&[u8]> = merged.split(|&b| b == b':').collect();
    assert_eq!(entries[0], b"/opt/custom/bin");
    assert!(entries.contains(&bad_path_entry.as_bytes()));
    assert_eq!(entries.iter().filter(|e| **e == b"/usr/bin").count(), 1);
    assert!(entries.contains(&&b"/Users/someone/.local/bin"[..]));
    // HOME is supplied when the inherited environment has none.
    assert_eq!(
        env.get(std::ffi::OsStr::new("HOME")),
        Some(&os("/Users/someone"))
    );
}
