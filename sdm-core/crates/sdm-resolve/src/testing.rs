//! Test doubles for the process seam. Compiled only with the `testing`
//! feature; dependents enable it from their `[dev-dependencies]`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;

use crate::{ProcessOutput, ProcessRunError, ProcessRunner};

/// Records the arguments it was called with and replays a scripted output.
pub struct FakeProcessRunner {
    calls: Mutex<Vec<Call>>,
    /// Matched against the joined argument string; first match wins.
    pub responses: Vec<(String, Result<ProcessOutput, ProcessRunError>)>,
    pub default_output: Result<ProcessOutput, ProcessRunError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
}

impl FakeProcessRunner {
    pub fn new() -> Self {
        FakeProcessRunner {
            calls: Mutex::new(Vec::new()),
            responses: Vec::new(),
            default_output: Ok(ProcessOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit_code: 0,
            }),
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for FakeProcessRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessRunner for FakeProcessRunner {
    async fn run(
        &self,
        executable: &Path,
        arguments: &[String],
        _timeout: Duration,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.calls.lock().unwrap().push(Call {
            executable: executable.to_path_buf(),
            arguments: arguments.to_vec(),
        });
        let joined = arguments.join(" ");
        self.responses
            .iter()
            .find(|(m, _)| joined.contains(m.as_str()))
            .map(|(_, o)| o.clone())
            .unwrap_or_else(|| self.default_output.clone())
    }
}

/// A successful (exit 0) output with the given stdout and empty stderr.
pub fn ok(stdout: impl Into<Vec<u8>>) -> Result<ProcessOutput, ProcessRunError> {
    ok_with_stderr(stdout, Vec::new())
}

pub fn ok_with_stderr(
    stdout: impl Into<Vec<u8>>,
    stderr: impl Into<Vec<u8>>,
) -> Result<ProcessOutput, ProcessRunError> {
    Ok(ProcessOutput {
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit_code: 0,
    })
}

/// A completed run that exited non-zero with the given stderr.
pub fn fail(stderr: &str, exit_code: i32) -> Result<ProcessOutput, ProcessRunError> {
    Ok(ProcessOutput {
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
        exit_code,
    })
}

/// `fail` with Swift's default exit code of 1.
pub fn fail_default(stderr: &str) -> Result<ProcessOutput, ProcessRunError> {
    fail(stderr, 1)
}
