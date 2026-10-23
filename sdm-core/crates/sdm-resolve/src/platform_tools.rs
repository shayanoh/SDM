//! The pinned downloads for `ffmpeg`, `ffprobe` and `qjs` on Windows x64 and
//! Linux x86_64 (macOS bundles its own; see `ManagedBinaries`).
//!
//! Pins recorded 2026-10-24. Every value was checked against the publisher's
//! own checksum list and against a download that was inspected by hand.
//!
//! | Tool | Source | Asset | Size | SHA-256 source |
//! |---|---|---|---|---|
//! | ffmpeg, ffprobe (Windows) | BtbN/FFmpeg-Builds `autobuild-2026-10-23-13-04` | `ffmpeg-n9.0.2-35-g9a9b70bb8c-win64-gpl-9.0.zip`, members `…/bin/ffmpeg.exe`, `…/bin/ffprobe.exe` | 194 MB | the release's `checksums.sha256` |
//! | ffmpeg, ffprobe (Linux) | same release | `ffmpeg-n9.0.2-35-g9a9b70bb8c-linux64-gpl-9.0.tar.xz`, members `…/bin/ffmpeg`, `…/bin/ffprobe` | 151 MB | the release's `checksums.sha256` |
//! | qjs (Windows) | quickjs-ng/quickjs `v0.17.0` | `qjs-windows-x86_64.exe` (a single executable) | 2.1 MB | the GitHub release asset digest, matched by a local hash |
//! | qjs (Linux) | quickjs-ng/quickjs `v0.17.0` | `qjs-linux-x86_64` (a single executable) | 2.6 MB | same |
//!
//! BtbN keeps only its recent dated autobuilds, so this pin eventually 404s.
//! The fix is one edit: choose a newer `autobuild-…` tag, download the two
//! archives, copy the checksums from that release's `checksums.sha256`, and
//! update `FFMPEG_VERSION` and the file names below. Never pin `latest`.
//!
//! The GPL builds are downloaded by the user's machine at run time; SDM does
//! not redistribute them (decision T7).

/// How a download is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// The download is the executable itself.
    Raw,
    Zip,
    TarXz,
}

/// One file taken out of a download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMember {
    /// The path inside the archive (empty for [`ArchiveKind::Raw`]).
    pub path_in_archive: String,
    /// The file name written into `bin/`.
    pub file_name: String,
}

impl ToolMember {
    pub fn new(path_in_archive: impl Into<String>, file_name: impl Into<String>) -> Self {
        Self {
            path_in_archive: path_in_archive.into(),
            file_name: file_name.into(),
        }
    }
}

/// One pinned download: a version, where it comes from, its SHA-256 and the
/// files it provides. `ffmpeg` and `ffprobe` share one download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTool {
    /// `"ffmpeg"` (provides `ffmpeg` and `ffprobe`) or `"qjs"`.
    pub name: String,
    /// What the manifest records once installed.
    pub version: String,
    pub url: String,
    /// Lowercase hex.
    pub sha256: String,
    pub archive: ArchiveKind,
    pub members: Vec<ToolMember>,
}

const FFMPEG_VERSION: &str = "n9.0.2-35-g9a9b70bb8c";
const FFMPEG_RELEASE: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-23-13-04";
const QJS_VERSION: &str = "v0.17.0";
const QJS_RELEASE: &str = "https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0";

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_x64_tools() -> Vec<PlatformTool> {
    let dir = format!("ffmpeg-{FFMPEG_VERSION}-win64-gpl-9.0");
    vec![
        PlatformTool {
            name: "ffmpeg".into(),
            version: FFMPEG_VERSION.into(),
            url: format!("{FFMPEG_RELEASE}/{dir}.zip"),
            sha256: "bc5c5d45a5bb24bb8b7e83eb6ca8ed9373b85098fbdde1e2951a48f91d91b54a".into(),
            archive: ArchiveKind::Zip,
            members: vec![
                ToolMember::new(format!("{dir}/bin/ffmpeg.exe"), "ffmpeg.exe"),
                ToolMember::new(format!("{dir}/bin/ffprobe.exe"), "ffprobe.exe"),
            ],
        },
        PlatformTool {
            name: "qjs".into(),
            version: QJS_VERSION.into(),
            url: format!("{QJS_RELEASE}/qjs-windows-x86_64.exe"),
            sha256: "2aeabf0092c3262d6b2609824418f7dd7ed1f1df939f73b2b15645230cac0d77".into(),
            archive: ArchiveKind::Raw,
            members: vec![ToolMember::new("", "qjs.exe")],
        },
    ]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn linux_x64_tools() -> Vec<PlatformTool> {
    let dir = format!("ffmpeg-{FFMPEG_VERSION}-linux64-gpl-9.0");
    vec![
        PlatformTool {
            name: "ffmpeg".into(),
            version: FFMPEG_VERSION.into(),
            url: format!("{FFMPEG_RELEASE}/{dir}.tar.xz"),
            sha256: "143e31ca53d65d95bde12cc861e331fd2275e48a6ca988e62c46e8f5b77b9811".into(),
            archive: ArchiveKind::TarXz,
            members: vec![
                ToolMember::new(format!("{dir}/bin/ffmpeg"), "ffmpeg"),
                ToolMember::new(format!("{dir}/bin/ffprobe"), "ffprobe"),
            ],
        },
        PlatformTool {
            name: "qjs".into(),
            version: QJS_VERSION.into(),
            url: format!("{QJS_RELEASE}/qjs-linux-x86_64"),
            sha256: "0bfc02511a9f549c28b53880d988fc7cd5d361e90c5e8afdfcd7dc6774ceace5".into(),
            archive: ArchiveKind::Raw,
            members: vec![ToolMember::new("", "qjs")],
        },
    ]
}

/// The downloads for the platform this build runs on; empty elsewhere
/// (macOS gets its tools from the app bundle, other targets are unsupported).
pub fn platform_tools() -> Vec<PlatformTool> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return windows_x64_tools();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return linux_x64_tools();
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )))]
    return Vec::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<PlatformTool> {
        let mut tools = windows_x64_tools();
        tools.extend(linux_x64_tools());
        tools
    }

    #[test]
    fn platform_tools_are_pinned() {
        for tool in all() {
            assert!(!tool.url.contains("/latest"), "{} floats", tool.url);
            assert!(tool.url.starts_with("https://github.com/"), "{}", tool.url);
            assert_eq!(tool.sha256.len(), 64, "{}", tool.url);
            assert!(
                tool.sha256
                    .chars()
                    .all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "{}",
                tool.url
            );
            assert!(!tool.version.is_empty());
            assert!(!tool.members.is_empty());
            for member in &tool.members {
                assert_eq!(
                    member.path_in_archive.is_empty(),
                    tool.archive == ArchiveKind::Raw,
                    "{}",
                    member.file_name
                );
            }
        }
    }

    #[test]
    fn each_platform_provides_ffmpeg_ffprobe_and_qjs() {
        for (tools, exe) in [(windows_x64_tools(), ".exe"), (linux_x64_tools(), "")] {
            let mut names: Vec<String> = tools
                .iter()
                .flat_map(|t| t.members.iter().map(|m| m.file_name.clone()))
                .collect();
            names.sort();
            assert_eq!(
                names,
                vec![
                    format!("ffmpeg{exe}"),
                    format!("ffprobe{exe}"),
                    format!("qjs{exe}")
                ]
            );
        }
    }
}
