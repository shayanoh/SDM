#[test]
fn core_version_names_every_crate() {
    let v = sdm_ffi::core_version();
    assert_eq!(v.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        v.crates,
        vec![
            "sdm-core",
            "sdm-resolve",
            "sdm-engine",
            "sdm-grabber",
            "sdm-ffi"
        ]
    );
}

#[tokio::test]
async fn echo_returns_its_input() {
    assert_eq!(sdm_ffi::core_echo_after_yield("hi".into()).await, "hi");
}
