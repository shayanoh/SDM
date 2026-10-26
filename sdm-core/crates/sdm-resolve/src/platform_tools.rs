//! Where `ffmpeg`, `ffprobe` and `qjs` come from on Windows x64 and Linux
//! x86_64 (macOS bundles its own; see `ManagedBinaries`).
//!
//! Nothing here is pinned to a build: each tool names a GitHub release and an
//! asset, and the managed binaries resolve the newest one when they need it
//! (decision of 2026-10-24: a pinned build expires when its publisher
//! deletes it, and keeping a pin fresh is upkeep this app should not need).
//! The checksum comes from the release itself (the asset `digest` GitHub
//! publishes), the same trust `yt-dlp`'s update already places in its release.
//!
//! | Tool | Release | Asset | Updated |
//! |---|---|---|---|
//! | ffmpeg, ffprobe | BtbN/FFmpeg-Builds, the rolling `latest` tag | `ffmpeg-master-latest-win64-gpl.zip`, `ffmpeg-master-latest-linux64-gpl.tar.xz` | only when missing (the build is rebuilt daily and is 150 to 200 MB) |
//! | qjs | quickjs-ng/quickjs, the newest release | `qjs-windows-x86_64.exe`, `qjs-linux-x86_64` | when the release tag changes (2 MB) |
//!
//! The GPL builds are downloaded by the user's machine at run time; SDM does
//! not redistribute them (decision T7). The `master` builds are used because
//! their asset names never change; the numbered ones are renamed at every
//! FFmpeg release.

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
    /// Which archive entry: the entry's path must equal this or end with `/` and
    /// this (the top folder of a build changes name with every build). Empty for
    /// [`ArchiveKind::Raw`].
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

/// When an installed tool is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePolicy {
    /// Only when a file is missing. No network at all otherwise.
    IfMissing,
    /// When the release's version differs from the recorded one.
    WhenVersionChanges,
}

/// What the recorded version is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    /// The release's tag (`v0.17.0`).
    Tag,
    /// The day the release was published (`2026-10-23`), for a rolling tag.
    PublishedDate,
}

/// A download SDM keeps installed: which release asset, how it is packed and
/// which files to take from it. `ffmpeg` and `ffprobe` share one download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTool {
    /// `"ffmpeg"` (provides `ffmpeg` and `ffprobe`) or `"qjs"`; the manifest key.
    pub name: String,
    /// The GitHub API address of the release to read.
    pub release_api: String,
    /// The asset's exact name in that release.
    pub asset_name: String,
    pub archive: ArchiveKind,
    pub members: Vec<ToolMember>,
    pub policy: UpdatePolicy,
    pub version_source: VersionSource,
}

const FFMPEG_RELEASE: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest";
const QJS_RELEASE: &str = "https://api.github.com/repos/quickjs-ng/quickjs/releases/latest";

fn ffmpeg(asset: &str, archive: ArchiveKind, exe: &str) -> PlatformTool {
    PlatformTool {
        name: "ffmpeg".into(),
        release_api: FFMPEG_RELEASE.into(),
        asset_name: asset.into(),
        archive,
        members: vec![
            ToolMember::new(format!("bin/ffmpeg{exe}"), format!("ffmpeg{exe}")),
            ToolMember::new(format!("bin/ffprobe{exe}"), format!("ffprobe{exe}")),
        ],
        policy: UpdatePolicy::IfMissing,
        version_source: VersionSource::PublishedDate,
    }
}

fn qjs(asset: &str, file_name: &str) -> PlatformTool {
    PlatformTool {
        name: "qjs".into(),
        release_api: QJS_RELEASE.into(),
        asset_name: asset.into(),
        archive: ArchiveKind::Raw,
        members: vec![ToolMember::new("", file_name)],
        policy: UpdatePolicy::WhenVersionChanges,
        version_source: VersionSource::Tag,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_x64_tools() -> Vec<PlatformTool> {
    vec![
        ffmpeg(
            "ffmpeg-master-latest-win64-gpl.zip",
            ArchiveKind::Zip,
            ".exe",
        ),
        qjs("qjs-windows-x86_64.exe", "qjs.exe"),
    ]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn linux_x64_tools() -> Vec<PlatformTool> {
    vec![
        ffmpeg(
            "ffmpeg-master-latest-linux64-gpl.tar.xz",
            ArchiveKind::TarXz,
            "",
        ),
        qjs("qjs-linux-x86_64", "qjs"),
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
    fn nothing_is_pinned_to_a_build() {
        let allowed = [
            "ffmpeg-master-latest-win64-gpl.zip",
            "ffmpeg-master-latest-linux64-gpl.tar.xz",
            "qjs-windows-x86_64.exe",
            "qjs-linux-x86_64",
        ];
        for tool in all() {
            assert!(
                tool.release_api.ends_with("/releases/latest")
                    || tool.release_api.ends_with("/releases/tags/latest"),
                "{}",
                tool.release_api
            );
            // Asset names carry no version or date, or they would expire.
            assert!(
                allowed.contains(&tool.asset_name.as_str()),
                "{}",
                tool.asset_name
            );
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

    #[test]
    fn ffmpeg_is_fetched_only_when_missing_and_qjs_follows_its_release() {
        for tools in [windows_x64_tools(), linux_x64_tools()] {
            assert_eq!(tools[0].policy, UpdatePolicy::IfMissing);
            assert_eq!(tools[1].policy, UpdatePolicy::WhenVersionChanges);
        }
    }
}
