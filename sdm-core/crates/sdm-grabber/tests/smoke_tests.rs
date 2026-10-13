#[test]
fn grabber_target_builds() {
    // Linking `sdm_grabber` at all is the check.
    let _ = sdm_grabber::GrabberSnapshot::new(vec![], vec![], vec![], 0, 0);
}
