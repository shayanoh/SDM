//! Helpers shared by the sdm-resolve integration tests (the fixture loaders
//! from the Swift `Support.swift`; the process-runner doubles live in
//! `sdm_resolve::testing`).

#![allow(dead_code)]

use std::path::PathBuf;

use sdm_resolve::YtDlpDump;

pub fn fixture_data(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {name}.json: {e}"))
}

pub fn fixture_dump(name: &str) -> YtDlpDump {
    YtDlpDump::from_json(&fixture_data(name))
        .unwrap_or_else(|e| panic!("fixture {name}.json does not decode: {e}"))
}
