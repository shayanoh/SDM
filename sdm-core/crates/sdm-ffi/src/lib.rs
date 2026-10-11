//! The UniFFI surface: staticlib + cdylib consumed by the Swift package.

use sdm_core::CoreVersion;

uniffi::setup_scaffolding!();

/// Temporary skeleton function (replaced when the real surface is bound).
#[uniffi::export]
pub fn core_version() -> CoreVersion {
    CoreVersion {
        version: env!("CARGO_PKG_VERSION").to_string(),
        crates: [
            "sdm-core",
            "sdm-resolve",
            "sdm-engine",
            "sdm-grabber",
            "sdm-ffi",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect(),
    }
}

/// Temporary skeleton async function: runs on the tokio runtime and yields once.
#[uniffi::export(async_runtime = "tokio")]
pub async fn core_echo_after_yield(text: String) -> String {
    tokio::task::yield_now().await;
    text
}
