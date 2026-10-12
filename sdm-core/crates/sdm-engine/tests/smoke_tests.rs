/// Swift: `#expect(Bool(true))` — the target links and the test binary runs.
#[test]
fn engine_target_builds() {
    let _ = sdm_engine::RetryPolicy::new();
}
