//! Helpers shared by the sdm-grabber integration tests (the `vf` / `af`
//! format builders from the Swift `FakeLinkResolver.swift`, plus a
//! `SourceUrl` shorthand). Each test file uses only some of them.

#![allow(dead_code, unused_imports)]

mod fake_link_resolver;
mod loopback_server;

pub use fake_link_resolver::{FakeLinkResolver, single_media};
pub use loopback_server::{
    LoopbackBody, LoopbackRequest, LoopbackResponse, LoopbackServer, payload_handler,
};

use sdm_core::{AudioCodec, MediaContainer, MediaFormat, MediaKind, SourceUrl, VideoCodec};

pub fn su(s: &str) -> SourceUrl {
    SourceUrl::new(s).expect("test URL parses")
}

/// A video-only format 1000 bytes large, `h * 16 / 9` wide, at `https://gv/<id>`.
pub fn vf(id: &str, h: i64, v: VideoCodec, c: MediaContainer) -> MediaFormat {
    vf_sized(id, h, v, c, 1000)
}

pub fn vf_sized(id: &str, h: i64, v: VideoCodec, c: MediaContainer, size: i64) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::VideoOnly,
        Some(h),
        Some(h * 16 / 9),
        Some(v),
        None,
        c,
        Some(size),
        None,
        Some(1000.0),
        su(&format!("https://gv/{id}")),
    )
}

/// An audio-only format 100 bytes large at `https://gv/<id>`.
pub fn af(id: &str, a: AudioCodec, c: MediaContainer) -> MediaFormat {
    MediaFormat::new(
        id,
        MediaKind::AudioOnly,
        None,
        None,
        None,
        Some(a),
        c,
        Some(100),
        None,
        Some(128.0),
        su(&format!("https://gv/{id}")),
    )
}

/// How long a loopback test may take before it fails instead of hanging.
pub const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Fails the test, rather than hanging the suite, if `future` takes longer
/// than [`TEST_TIMEOUT`].
pub async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(TEST_TIMEOUT, future)
        .await
        .expect("timed out: the test hung")
}

/// Polls `condition` (yielding between looks) until it holds. Unbounded on
/// its own: call it inside [`within`], whose timeout turns a hang into a
/// failure.
pub async fn wait_until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::task::yield_now().await;
    }
}

/// A clock the test moves by hand; stands in for `Date()`.
#[derive(Clone)]
pub struct ManualClock(std::sync::Arc<std::sync::Mutex<std::time::SystemTime>>);

impl ManualClock {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000),
        )))
    }

    pub fn now(&self) -> std::time::SystemTime {
        *self.0.lock().unwrap()
    }

    pub fn advance(&self, by: std::time::Duration) {
        *self.0.lock().unwrap() += by;
    }

    pub fn provider(&self) -> impl Fn() -> std::time::SystemTime + Send + Sync + 'static {
        let clone = self.clone();
        move || clone.now()
    }
}
