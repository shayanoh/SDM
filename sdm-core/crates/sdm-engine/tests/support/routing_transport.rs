use async_trait::async_trait;
use sdm_engine::{
    FakeOrigin, FakeOriginBehavior, HttpTransport, RangeRequest, RangeResponse, TransportError,
};

/// Dispatches a fetch to one of two `FakeOrigin`s by request host, so a
/// multi-component item whose parts live on different hosts can be driven in
/// one test. An unknown host goes to the video origin.
pub struct RoutingTransport {
    pub video: FakeOrigin,
    pub audio: FakeOrigin,
    video_host: String,
    audio_host: String,
}

impl RoutingTransport {
    pub fn new(
        video_host: &str,
        video_payload: Vec<u8>,
        video_behavior: FakeOriginBehavior,
        audio_host: &str,
        audio_payload: Vec<u8>,
        audio_behavior: FakeOriginBehavior,
    ) -> Self {
        Self {
            video: FakeOrigin::with_behavior(video_payload, video_behavior),
            audio: FakeOrigin::with_behavior(audio_payload, audio_behavior),
            video_host: video_host.to_string(),
            audio_host: audio_host.to_string(),
        }
    }
}

#[async_trait]
impl HttpTransport for RoutingTransport {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        match request.url.host_str() {
            Some(host) if host == self.audio_host => self.audio.fetch(request).await,
            Some(host) if host == self.video_host => self.video.fetch(request).await,
            _ => self.video.fetch(request).await,
        }
    }
}

/// Dispatches a fetch to one of any number of `FakeOrigin`s by request host,
/// for tests that need a different payload per item. An unknown host fails the
/// fetch with a 404-shaped transport error rather than serving the wrong
/// bytes. Cloning shares the same origins.
#[derive(Clone, Default)]
pub struct HostTransport {
    origins: std::sync::Arc<std::collections::HashMap<String, FakeOrigin>>,
}

impl HostTransport {
    pub fn new(origins: impl IntoIterator<Item = (String, FakeOrigin)>) -> Self {
        Self {
            origins: std::sync::Arc::new(origins.into_iter().collect()),
        }
    }
}

#[async_trait]
impl HttpTransport for HostTransport {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let origin = request
            .url
            .host_str()
            .and_then(|host| self.origins.get(host))
            .unwrap_or_else(|| panic!("no origin for {}", request.url));
        origin.fetch(request).await
    }
}
