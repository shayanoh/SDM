use sdm_resolve::{BinariesManifest, YtDlpChannel, YtDlpVersion};

#[test]
fn version_compares_component_wise() {
    let v = |s: &str| YtDlpVersion::new(s).unwrap();
    assert!(v("2026.09.01") > v("2026.08.19"));
    assert!(v("2026.08.19.120000") > v("2026.08.19"));
    assert!(v("2026.08.19") == v("2026.08.19"));
    assert!(v("2027.01.01") > v("2026.12.31"));
    assert!(YtDlpVersion::new("garbage").is_none());
    assert!(YtDlpVersion::new("").is_none());
}

#[test]
fn manifest_round_trips() {
    let mut m = BinariesManifest::empty();
    m.yt_dlp_version = Some("2026.08.19".into());
    m.yt_dlp_channel = YtDlpChannel::Nightly;
    m.last_error = Some("boom".into());
    m.ffmpeg_version = Some("7.1".into());
    let data = serde_json::to_vec(&m).unwrap();
    assert_eq!(
        serde_json::from_slice::<BinariesManifest>(&data).unwrap(),
        m
    );
}

#[test]
fn manifest_version_lookup_by_name() {
    let mut m = BinariesManifest::empty();
    m.ffmpeg_version = Some("7.1".into());
    m.qjs_version = Some("0.10.0".into());
    assert_eq!(m.version_for("ffmpeg"), Some("7.1"));
    assert_eq!(m.version_for("ffprobe"), Some("7.1"));
    assert_eq!(m.version_for("qjs"), Some("0.10.0"));
    assert_eq!(m.version_for("yt-dlp"), None);
}

#[test]
fn channel_picks_the_right_repo() {
    assert_eq!(
        YtDlpChannel::Stable.releases_api().as_str(),
        "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest"
    );
    assert_eq!(
        YtDlpChannel::Nightly.releases_api().as_str(),
        "https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest"
    );
    assert_eq!(
        YtDlpChannel::Stable.asset_url("2026.08.19").as_str(),
        "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos"
    );
    assert_eq!(
        YtDlpChannel::Stable.sums_url("2026.08.19").as_str(),
        "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/SHA2-256SUMS"
    );
}

fn swift_fixture() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/swift/binaries_manifest.json");
    std::fs::read(path).unwrap()
}

/// The Swift app and this core share one `manifest.json`. `FixtureDump`
/// writes the file with `ManagedBinaries`' encoder settings, then appends a
/// single newline that `saveManifest` itself does not write.
#[test]
fn manifest_matches_the_swift_fixture_byte_for_byte() {
    let fixture = swift_fixture();
    let (body, newline) = fixture.split_at(fixture.len() - 1);
    assert_eq!(newline, b"\n");

    let manifest: BinariesManifest = serde_json::from_slice(&fixture).unwrap();
    assert_eq!(
        manifest.yt_dlp_version.as_deref(),
        Some("2026.09.01.123456")
    );
    assert_eq!(manifest.yt_dlp_channel, YtDlpChannel::Nightly);
    assert_eq!(manifest.last_check_tick, 4242);
    assert_eq!(manifest.ffmpeg_version.as_deref(), Some("7.1"));
    assert_eq!(manifest.qjs_version.as_deref(), Some("2025-09-13"));
    assert_eq!(manifest.last_error.as_deref(), Some("offline: timed out"));

    assert_eq!(
        String::from_utf8(manifest.to_foundation_json()).unwrap(),
        String::from_utf8(body.to_vec()).unwrap()
    );
}

/// Foundation's `JSONEncoder` escapes `/` as `\/`, writes other control
/// characters as lowercase `\u00xx`, leaves non-ASCII raw, omits `nil`
/// optionals and sorts keys. Expected bytes captured from a real
/// `JSONEncoder` (`.prettyPrinted`, `.sortedKeys`).
#[test]
fn manifest_json_escapes_like_foundation() {
    let mut m = BinariesManifest::empty();
    m.last_error =
        Some("q\"b\\s/c\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}\u{85}\u{2028}é😀\u{0}end".into());
    let expected = "{\n  \"lastCheckTick\" : 0,\n  \"lastError\" : \"q\\\"b\\\\s\\/c\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}\u{85}\u{2028}é😀\\u0000end\",\n  \"ytDlpChannel\" : \"stable\"\n}";
    assert_eq!(String::from_utf8(m.to_foundation_json()).unwrap(), expected);
}
