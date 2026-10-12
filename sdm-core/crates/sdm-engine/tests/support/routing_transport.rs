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
