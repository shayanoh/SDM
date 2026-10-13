use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{ProcessOutput, ProcessRunError, ProcessRunner};

/// `tokio::process` implementation. Unsandboxed target — this is
/// allowed. On timeout the child is terminated; on cancellation (the
/// `CancellationToken`) it is sent SIGTERM and given a bounded window to
/// exit. Dropping the future is only a backstop: the child is then
/// SIGTERMed and reaped on a background task.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcessRunner;

type ChunkSink = Arc<dyn Fn(&[u8]) + Send + Sync>;

impl SystemProcessRunner {
    pub fn new() -> Self {
        SystemProcessRunner
    }

    /// A `.app` launched from Finder inherits a near-empty `PATH` and may be
    /// missing `HOME`. yt-dlp needs `HOME` for its cache/config and shells
    /// out to `ffmpeg` and a JS runtime (`deno`/`node`) for YouTube's nsig
    /// challenge — without those on `PATH` its extraction degrades and
    /// YouTube starts returning anti-bot walls. So we hand the child a real
    /// environment: whatever we inherited, plus the standard tool
    /// directories (and the executable's own directory) prepended to `PATH`,
    /// and `HOME` guaranteed.
    ///
    /// Names and values are carried as `OsString`s: the process environment
    /// is not required to be UTF-8, and nothing in it is skipped or altered.
    ///
    /// Public (hidden) only so the integration tests can reach it.
    #[doc(hidden)]
    pub fn child_environment(executable: &Path) -> HashMap<OsString, OsString> {
        Self::child_environment_from(executable, std::env::vars_os(), home_directory())
    }

    /// [`Self::child_environment`] over an explicit inherited environment and
    /// home directory, so tests need not mutate the process's own.
    #[doc(hidden)]
    pub fn child_environment_from(
        executable: &Path,
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
        home: PathBuf,
    ) -> HashMap<OsString, OsString> {
        let mut env: HashMap<OsString, OsString> = inherited.into_iter().collect();

        let mut tool_dirs: Vec<PathBuf> = vec![
            executable
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ];
        for dir in [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/opt/local/bin",
            "/opt/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ] {
            tool_dirs.push(PathBuf::from(dir));
        }
        let mut home_bin = home.as_os_str().as_bytes().to_vec();
        while home_bin.ends_with(b"/") {
            home_bin.pop();
        }
        home_bin.extend_from_slice(b"/.local/bin");
        tool_dirs.push(PathBuf::from(OsString::from_vec(home_bin)));

        let existing: Vec<PathBuf> = env
            .get(OsStr::new("PATH"))
            .map(|p| {
                p.as_bytes()
                    .split(|&b| b == b':')
                    .filter(|s| !s.is_empty())
                    .map(|s| PathBuf::from(OsStr::from_bytes(s)))
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let merged: Vec<PathBuf> = tool_dirs
            .into_iter()
            .chain(existing)
            .filter(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()))
            .collect();
        let mut path = Vec::new();
        for (index, dir) in merged.iter().enumerate() {
            if index > 0 {
                path.push(b':');
            }
            path.extend_from_slice(dir.as_os_str().as_bytes());
        }
        env.insert(OsString::from("PATH"), OsString::from_vec(path));

        if env.get(OsStr::new("HOME")).is_none_or(|h| h.is_empty()) {
            env.insert(OsString::from("HOME"), home.into_os_string());
        }
        env
    }
}

#[async_trait]
impl ProcessRunner for SystemProcessRunner {
    async fn run(
        &self,
        executable: &Path,
        arguments: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, ProcessRunError> {
        // Read the pipes on background tasks so a large output can never
        // deadlock against a full pipe buffer while the process runs.
        let stdout = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr = Arc::new(Mutex::new(Vec::<u8>::new()));
        let out_sink = stdout.clone();
        let err_sink = stderr.clone();
        let exit_code = execute(
            executable,
            arguments,
            timeout,
            cancel,
            Arc::new(move |chunk| out_sink.lock().unwrap().extend_from_slice(chunk)),
            Arc::new(move |chunk| err_sink.lock().unwrap().extend_from_slice(chunk)),
        )
        .await?;
        let stdout = std::mem::take(&mut *stdout.lock().unwrap());
        let stderr = std::mem::take(&mut *stderr.lock().unwrap());
        Ok(ProcessOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn run_streaming(
        &self,
        executable: &Path,
        arguments: &[String],
        timeout: Duration,
        on_line: Arc<dyn Fn(String) + Send + Sync>,
        cancel: &CancellationToken,
    ) -> Result<i32, ProcessRunError> {
        let streamer = Arc::new(LineStreamer::new(on_line));
        let out_streamer = streamer.clone();
        let err_streamer = streamer.clone();
        let exit_code = execute(
            executable,
            arguments,
            timeout,
            cancel,
            Arc::new(move |chunk| out_streamer.feed(chunk)),
            Arc::new(move |chunk| err_streamer.feed(chunk)),
        )
        .await?;
        streamer.flush();
        Ok(exit_code)
    }
}

/// Spawns the child, pumps both pipes into the sinks, and races its exit
/// against `timeout` and `cancel`. Cancellation sends SIGTERM, gives the
/// child a bounded window to exit, and returns `Cancelled`.
async fn execute(
    executable: &Path,
    arguments: &[String],
    timeout: Duration,
    cancel: &CancellationToken,
    on_stdout: ChunkSink,
    on_stderr: ChunkSink,
) -> Result<i32, ProcessRunError> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .envs(SystemProcessRunner::child_environment(executable))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| ProcessRunError::LaunchFailed(e.to_string()))?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let mut guard = ChildGuard { child: Some(child) };

    let stop_out = Arc::new(Notify::new());
    let stop_err = Arc::new(Notify::new());
    let pumps = [
        tokio::spawn(pump(stdout, stop_out.clone(), on_stdout)),
        tokio::spawn(pump(stderr, stop_err.clone(), on_stderr)),
    ];

    // Swift launches before looking at cancellation, so an already-cancelled
    // token launches the child and then terminates it straight away.
    let child = guard.child.as_mut().expect("child present");
    let exited: Option<std::io::Result<ExitStatus>> = tokio::select! {
        biased;
        status = child.wait() => Some(status),
        _ = cancel.cancelled() => None,
        _ = tokio::time::sleep(timeout) => {
            // Timed out: terminate, then wait for the child to be gone.
            guard.terminate();
            let child = guard.child.as_mut().expect("child present");
            let _ = child.wait().await;
            guard.child = None;
            stop_and_join(&stop_out, &stop_err, pumps).await;
            return Err(ProcessRunError::TimedOut);
        }
    };

    let result = match exited {
        Some(_) if cancel.is_cancelled() => {
            // Exited, but cancelled before we looked (`Task.isCancelled`).
            drain_after_terminate(&mut guard).await;
            Err(ProcessRunError::Cancelled)
        }
        Some(status) => status
            .map(|s| s.code().or_else(|| s.signal()).unwrap_or(-1))
            .map_err(|e| ProcessRunError::LaunchFailed(e.to_string())),
        None => {
            guard.terminate();
            drain_after_terminate(&mut guard).await;
            Err(ProcessRunError::Cancelled)
        }
    };
    guard.child = None;
    stop_and_join(&stop_out, &stop_err, pumps).await;
    result
}

/// How long a cancelled child gets to exit on its own after SIGTERM so it
/// can flush resume state (yt-dlp's `.ytdl` / `.part`) before a rescheduled
/// run touches the same files.
const DRAIN_AFTER_TERMINATE: Duration = Duration::from_secs(3);

/// Waits up to `DRAIN_AFTER_TERMINATE` for the child to exit. If it does
/// not, it is handed to a background task that reaps it (it is not killed
/// further, as in Swift).
async fn drain_after_terminate(guard: &mut ChildGuard) {
    let Some(child) = guard.child.as_mut() else {
        return;
    };
    if tokio::time::timeout(DRAIN_AFTER_TERMINATE, child.wait())
        .await
        .is_err()
        && let Some(mut child) = guard.child.take()
        && let Ok(handle) = tokio::runtime::Handle::try_current()
    {
        handle.spawn(async move {
            let _ = child.wait().await;
        });
    }
}

/// Closes the pipes: take whatever is already buffered, do not wait for EOF
/// (a grandchild may still hold the write end open).
async fn stop_and_join(stop_out: &Notify, stop_err: &Notify, pumps: [JoinHandle<()>; 2]) {
    stop_out.notify_one();
    stop_err.notify_one();
    for pump in pumps {
        let _ = pump.await;
    }
}

/// Reads `reader` into `sink` until EOF or until `stop` fires, after which
/// it takes only what is readable without blocking.
async fn pump<R: AsyncRead + Unpin>(mut reader: R, stop: Arc<Notify>, sink: ChunkSink) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            biased;
            _ = stop.notified() => break,
            n = reader.read(&mut buf) => match n {
                Ok(0) | Err(_) => return,
                Ok(n) => sink(&buf[..n]),
            },
        }
    }
    loop {
        // `ready` loses to `read` only when `read` is not immediately ready.
        let n = tokio::select! {
            biased;
            n = reader.read(&mut buf) => n,
            _ = std::future::ready(()) => return,
        };
        match n {
            Ok(0) | Err(_) => return,
            Ok(n) => sink(&buf[..n]),
        }
    }
}

/// Owns the child until it has been reaped. Backstop only: explicit
/// cancellation goes through the `CancellationToken`. If the future is
/// dropped anyway while the child is still running, SIGTERM it and reap it
/// in the background so it cannot linger as a zombie (the caller cannot
/// await anything on this path).
struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn terminate(&self) {
        if let Some(pid) = self.child.as_ref().and_then(Child::id) {
            // A pid that does not fit `pid_t` cannot be a real child: skip.
            let Ok(pid) = libc::pid_t::try_from(pid) else {
                return;
            };
            // SAFETY: `pid` is a child we have not yet reaped (`Child::id`
            // returns `None` once it has), so it cannot have been recycled.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.child.as_ref().and_then(Child::id).is_none() {
            return;
        }
        self.terminate();
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = child.wait().await;
            });
        }
    }
}

fn home_directory() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Splits a byte stream into `\n`-delimited lines, emitting each completed
/// line via `on_line`. Thread-safe for the pipe readers.
struct LineStreamer {
    buffer: Mutex<Vec<u8>>,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
}

impl LineStreamer {
    fn new(on_line: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        LineStreamer {
            buffer: Mutex::new(Vec::new()),
            on_line,
        }
    }

    fn feed(&self, chunk: &[u8]) {
        let lines: Vec<String> = {
            let mut buffer = self.buffer.lock().unwrap();
            buffer.extend_from_slice(chunk);
            let mut completed = Vec::new();
            while let Some(newline) = buffer.iter().position(|&b| b == 0x0A) {
                let line = String::from_utf8_lossy(&buffer[..newline])
                    .trim_matches('\r')
                    .to_string();
                completed.push(line);
                buffer.drain(..=newline);
            }
            completed
        };
        for line in lines {
            (self.on_line)(line);
        }
    }

    fn flush(&self) {
        let trailing = {
            let mut buffer = self.buffer.lock().unwrap();
            if buffer.is_empty() {
                None
            } else {
                let s = String::from_utf8_lossy(&buffer).into_owned();
                buffer.clear();
                Some(s)
            }
        };
        if let Some(trailing) = trailing
            && !trailing.is_empty()
        {
            (self.on_line)(trailing);
        }
    }
}
