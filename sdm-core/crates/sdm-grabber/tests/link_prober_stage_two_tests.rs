mod support;

use sdm_grabber::{
    FakeProbeOrigin, FakeProbeOriginBehavior, FileSignature, LinkProber, ProbeMethod,
};
use std::sync::Arc;
use support::{su, within};

const URL: &str = "https://example.com/movie.mp4";

fn origin_with(status_code: i64, body: Vec<u8>) -> Arc<FakeProbeOrigin> {
    let origin = Arc::new(FakeProbeOrigin::new());
    let behavior = FakeProbeOriginBehavior {
        status_code,
        body,
        ..FakeProbeOriginBehavior::default()
    };
    origin.set_behavior(behavior, &su(URL));
    origin
}

fn methods(origin: &FakeProbeOrigin) -> Vec<ProbeMethod> {
    origin.request_log().iter().map(|r| r.method).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sniff_captures_magic_bytes_for_a_successful_link() {
    within(async {
        let origin = origin_with(200, vec![0x50, 0x4B, 0x03, 0x04, 0x00, 0x00]);
        let link = LinkProber::new(origin.clone(), true).probe(&su(URL)).await;
        assert_eq!(link.sniffed_signature, Some(FileSignature::Zip));
        assert_eq!(methods(&origin), vec![ProbeMethod::Head, ProbeMethod::Get]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sniff_skipped_when_deep_sniff_disabled() {
    within(async {
        let origin = origin_with(200, vec![0x50, 0x4B, 0x03, 0x04]);
        let link = LinkProber::new(origin.clone(), false).probe(&su(URL)).await;
        assert_eq!(link.sniffed_signature, None);
        assert_eq!(methods(&origin), vec![ProbeMethod::Head]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sniff_skipped_when_status_is_not_successful() {
    within(async {
        let origin = origin_with(404, Vec::new());
        let link = LinkProber::new(origin.clone(), true).probe(&su(URL)).await;
        assert_eq!(link.sniffed_signature, None);
        assert_eq!(methods(&origin), vec![ProbeMethod::Head]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_deep_sniff_source_overrides_the_flag_on_every_probe() {
    within(async {
        let origin = origin_with(200, vec![0x50, 0x4B, 0x03, 0x04]);
        let on = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&on);
        let prober = LinkProber::new(origin.clone(), true)
            .with_deep_sniff_source(move || flag.load(std::sync::atomic::Ordering::SeqCst));

        let off = prober.probe(&su(URL)).await;
        assert_eq!(off.sniffed_signature, None, "the source says off");

        on.store(true, std::sync::atomic::Ordering::SeqCst);
        let sniffed = prober.probe(&su(URL)).await;
        assert_eq!(sniffed.sniffed_signature, Some(FileSignature::Zip));
    })
    .await;
}
