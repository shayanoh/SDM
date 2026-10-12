//! Not ported from Swift (the Swift suite never exercises the protocol
//! extension or the fake directly): pins the default `run_streaming`, the
//! scripted fake, and the exit code reported for a signalled child.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use sdm_resolve::testing::{FakeProcessRunner, fail, ok, ok_with_stderr};
use sdm_resolve::{ProcessRunner, SystemProcessRunner};

#[tokio::test]
async fn default_run_streaming_replays_stdout_then_stderr_lines() {
    let mut fake = FakeProcessRunner::new();
    fake.default_output = ok_with_stderr("a\n\nb\n", "e1\ne2");
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let code = fake
        .run_streaming(
            Path::new("/x"),
            &[],
            Duration::from_secs(1),
            Arc::new(move |l| sink.lock().unwrap().push(l)),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(code, 0);
    assert_eq!(*lines.lock().unwrap(), ["a", "b", "e1", "e2"]);
}

#[tokio::test]
async fn fake_records_calls_and_first_match_wins() {
    let mut fake = FakeProcessRunner::new();
    fake.responses = vec![
        ("--dump".to_string(), ok("one")),
        ("--dump".to_string(), ok("two")),
        ("boom".to_string(), fail("bad", 2)),
    ];
    let args = vec!["--dump".to_string(), "x".to_string()];
    let out = fake
        .run(
            Path::new("/bin/tool"),
            &args,
            Duration::from_secs(1),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.stdout, b"one");
    let out = fake
        .run(
            Path::new("/bin/tool"),
            &["boom".to_string()],
            Duration::from_secs(1),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!((out.exit_code, out.stderr), (2, b"bad".to_vec()));
    assert_eq!(fake.calls().len(), 2);
    assert_eq!(fake.calls()[0].arguments, args);
}

/// Matches `Process.terminationStatus`, which reports the signal number for
/// an uncaught signal.
#[tokio::test]
async fn signalled_child_reports_the_signal_number() {
    let out = SystemProcessRunner::new()
        .run(
            Path::new("/bin/sh"),
            &["-c".to_string(), "kill -9 $$".to_string()],
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.exit_code, 9);
}
