//! Finds the LZFSE blobs an app bundle ships (`ffmpeg-<arch>.lzfse`,
//! `ffprobe-<arch>.lzfse`, `qjs.lzfse`) and the versions recorded beside them
//! in `vendor-manifest.json`. Used on macOS, where the tools are bundled
//! rather than downloaded.

use std::path::Path;

use serde::Deserialize;

use crate::managed_binaries_types::VendorAsset;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VendorManifest {
    ffmpeg_version: Option<String>,
    qjs_version: Option<String>,
}

/// The blob name suffix of the architecture this build runs on.
pub fn host_vendor_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// The assets in `dir` for `arch` (`"arm64"` or `"x86_64"`). A missing or
/// unreadable manifest, a version of `"0"` (the checked-in placeholder) or a
/// missing blob each mean "nothing to inflate" for that tool, never an error.
pub fn vendor_assets(dir: &Path, arch: &str) -> Vec<VendorAsset> {
    let Ok(text) = std::fs::read_to_string(dir.join("vendor-manifest.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<VendorManifest>(&text) else {
        return Vec::new();
    };
    let usable = |version: Option<String>| version.filter(|v| !v.is_empty() && v != "0");
    let mut assets = Vec::new();
    let mut push = |name: &str, file: String, version: String| {
        let path = dir.join(file);
        if path.is_file() {
            assets.push(VendorAsset {
                name: name.to_owned(),
                compressed_path: path,
                version,
            });
        }
    };
    if let Some(version) = usable(manifest.ffmpeg_version) {
        push("ffmpeg", format!("ffmpeg-{arch}.lzfse"), version.clone());
        // Same build as ffmpeg; yt-dlp's HLS/DASH fixups need it.
        push("ffprobe", format!("ffprobe-{arch}.lzfse"), version);
    }
    if let Some(version) = usable(manifest.qjs_version) {
        push("qjs", "qjs.lzfse".to_owned(), version);
    }
    assets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(manifest: Option<&str>, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(text) = manifest {
            std::fs::write(dir.path().join("vendor-manifest.json"), text).unwrap();
        }
        for file in files {
            std::fs::write(dir.path().join(file), b"blob").unwrap();
        }
        dir
    }

    const MANIFEST: &str = r#"{"ffmpegVersion":"N-1","qjsVersion":"0.10.1"}"#;
    const ALL: &[&str] = &[
        "ffmpeg-arm64.lzfse",
        "ffmpeg-x86_64.lzfse",
        "ffprobe-arm64.lzfse",
        "ffprobe-x86_64.lzfse",
        "qjs.lzfse",
    ];

    fn summary(assets: &[VendorAsset]) -> Vec<(String, String)> {
        assets
            .iter()
            .map(|a| (a.name.clone(), a.version.clone()))
            .collect()
    }

    #[test]
    fn picks_the_blobs_of_the_requested_architecture() {
        let dir = dir_with(Some(MANIFEST), ALL);
        let arm = vendor_assets(dir.path(), "arm64");
        assert_eq!(
            summary(&arm),
            [("ffmpeg", "N-1"), ("ffprobe", "N-1"), ("qjs", "0.10.1")]
                .map(|(n, v)| (n.to_owned(), v.to_owned()))
        );
        assert!(arm[0].compressed_path.ends_with("ffmpeg-arm64.lzfse"));
        let intel = vendor_assets(dir.path(), "x86_64");
        assert!(intel[1].compressed_path.ends_with("ffprobe-x86_64.lzfse"));
    }

    #[test]
    fn the_placeholder_version_means_nothing_to_inflate() {
        let dir = dir_with(Some(r#"{"ffmpegVersion":"0","qjsVersion":"0"}"#), ALL);
        assert!(vendor_assets(dir.path(), "arm64").is_empty());
    }

    #[test]
    fn missing_manifest_or_garbage_gives_nothing() {
        assert!(vendor_assets(dir_with(None, ALL).path(), "arm64").is_empty());
        assert!(vendor_assets(dir_with(Some("not json"), ALL).path(), "arm64").is_empty());
        assert!(vendor_assets(Path::new("/definitely/not/here"), "arm64").is_empty());
    }

    #[test]
    fn a_missing_blob_skips_only_that_tool() {
        let dir = dir_with(Some(MANIFEST), &["qjs.lzfse", "ffmpeg-arm64.lzfse"]);
        let names: Vec<_> = vendor_assets(dir.path(), "arm64")
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, ["ffmpeg", "qjs"]);
    }
}
