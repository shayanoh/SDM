//! Port of `FakeMuxer` from Swift's `MuxAssemblyTests.swift`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sdm_core::MediaContainer;
use sdm_engine::{MuxError, Muxer};
use tokio_util::sync::CancellationToken;

type Behavior = Arc<dyn Fn(&Path) -> Result<(), MuxError> + Send + Sync>;

/// Records mux calls; scriptable to succeed (writing a stub output) or fail.
pub struct FakeMuxer {
    calls: Mutex<Vec<(PathBuf, PathBuf, PathBuf)>>,
    behavior: Mutex<Behavior>,
}

impl FakeMuxer {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            behavior: Mutex::new(Self::succeeding()),
        }
    }

    /// The default behavior: write a stub file at the output path.
    pub fn succeeding() -> Behavior {
        Arc::new(|output| {
            std::fs::write(output, b"MUXED").expect("write the stub output");
            Ok(())
        })
    }

    /// Replaces what the next `mux` calls do.
    pub fn set_behavior(
        &self,
        behavior: impl Fn(&Path) -> Result<(), MuxError> + Send + Sync + 'static,
    ) {
        *self.behavior.lock().unwrap() = Arc::new(behavior);
    }

    /// `(video, audio, output)` of every call, in order.
    pub fn calls(&self) -> Vec<(PathBuf, PathBuf, PathBuf)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for FakeMuxer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Muxer for FakeMuxer {
    async fn mux(
        &self,
        video_part: &Path,
        audio_part: &Path,
        output: &Path,
        _container: &MediaContainer,
        _cancel: &CancellationToken,
    ) -> Result<(), MuxError> {
        self.calls.lock().unwrap().push((
            video_part.to_path_buf(),
            audio_part.to_path_buf(),
            output.to_path_buf(),
        ));
        let behavior = self.behavior.lock().unwrap().clone();
        behavior(output)
    }
}
