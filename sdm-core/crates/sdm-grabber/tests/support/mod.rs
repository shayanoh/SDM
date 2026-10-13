//! Helpers shared by the sdm-grabber integration tests (the `vf` / `af`
//! format builders from the Swift `FakeLinkResolver.swift`, plus a
//! `SourceUrl` shorthand). Each test file uses only some of them.

#![allow(dead_code)]

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
