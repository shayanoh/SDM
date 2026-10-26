//! Where `ffmpeg`, `ffprobe` and `qjs` come from on every platform: Windows
//! x64, Linux x86_64 and macOS (Apple silicon and Intel). Nothing is bundled
//! with the app, so the three behave the same everywhere.
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
//! | ffmpeg, ffprobe (macOS) | ffmpeg.martin-riedl.de, the `latest/release` redirect (static, signed builds, one per CPU) | `ffmpeg.zip` and `ffprobe.zip` | only when missing |
//! | qjs | quickjs-ng/quickjs, the newest release | `qjs-windows-x86_64.exe`, `qjs-linux-x86_64`, `qjs-darwin-arm64`, `qjs-darwin-x86_64` | when the release tag changes (2 MB) |
//!
//! The GPL builds are downloaded by the user's machine at run time; SDM does
//! not redistribute them (decision T7). The `master` builds are used because
//! their asset names never change; the numbered ones are renamed at every
//! FFmpeg release. The macOS site publishes no checksum inside a release, so
//! the build the `latest` address redirects to is resolved first and the
//! `.sha256` file beside that build is the checksum.

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
    /// The build id in the address a [`ToolSource::Redirect`] lands on
    /// (`1789931890_9.0.2`).
    BuildId,
}

/// Where a tool's archive is downloaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSource {
    /// A GitHub release's asset, checked against the digest GitHub publishes.
    GitHub {
        /// The GitHub API address of the release to read.
        release_api: String,
        /// The asset's exact name in that release.
        asset_name: String,
    },
    /// A fixed address that redirects to the newest build. The build's own
    /// address plus `.sha256` holds the checksum.
    Redirect { latest_url: String },
}

/// A download SDM keeps installed: which release asset, how it is packed and
/// which files to take from it. `ffmpeg` and `ffprobe` share one download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTool {
    /// `"ffmpeg"` (provides `ffmpeg` and `ffprobe`) or `"qjs"`; the manifest key.
    pub name: String,
    pub source: ToolSource,
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
        source: ToolSource::GitHub {
            release_api: FFMPEG_RELEASE.into(),
            asset_name: asset.into(),
        },
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
        source: ToolSource::GitHub {
            release_api: QJS_RELEASE.into(),
            asset_name: asset.into(),
        },
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

const MARTIN_RIEDL_LATEST: &str = "https://ffmpeg.martin-riedl.de/redirect/latest/macos";

/// One of the two single-file zips the macOS site publishes (`tool` is
/// `ffmpeg` or `ffprobe`). Both carry the manifest name `ffmpeg`: they are one
/// build, and one recorded version.
fn macos_ffmpeg_part(site_arch: &str, tool: &str) -> PlatformTool {
    PlatformTool {
        name: "ffmpeg".into(),
        source: ToolSource::Redirect {
            latest_url: format!("{MARTIN_RIEDL_LATEST}/{site_arch}/release/{tool}.zip"),
        },
        archive: ArchiveKind::Zip,
        members: vec![ToolMember::new(tool, tool)],
        policy: UpdatePolicy::IfMissing,
        version_source: VersionSource::BuildId,
    }
}

/// `site_arch` is the macOS site's CPU name (`arm64`, `amd64`); `qjs_asset` the
/// matching quickjs-ng asset.
fn macos_tools(site_arch: &str, qjs_asset: &str) -> Vec<PlatformTool> {
    vec![
        macos_ffmpeg_part(site_arch, "ffmpeg"),
        macos_ffmpeg_part(site_arch, "ffprobe"),
        qjs(qjs_asset, "qjs"),
    ]
}

#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
pub(crate) fn macos_arm64_tools() -> Vec<PlatformTool> {
    macos_tools("arm64", "qjs-darwin-arm64")
}

#[cfg_attr(
    not(all(target_os = "macos", target_arch = "x86_64")),
    allow(dead_code)
)]
pub(crate) fn macos_x64_tools() -> Vec<PlatformTool> {
    macos_tools("amd64", "qjs-darwin-x86_64")
}

/// The downloads for the platform this build runs on; empty on an
/// unsupported one.
pub fn platform_tools() -> Vec<PlatformTool> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return windows_x64_tools();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return linux_x64_tools();
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return macos_arm64_tools();
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return macos_x64_tools();
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(
            target_os = "macos",
            any(target_arch = "aarch64", target_arch = "x86_64")
        )
    )))]
    return Vec::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<PlatformTool> {
        let mut tools = windows_x64_tools();
        tools.extend(linux_x64_tools());
        tools.extend(macos_arm64_tools());
        tools.extend(macos_x64_tools());
        tools
    }

    #[test]
    fn nothing_is_pinned_to_a_build() {
        let allowed = [
            "ffmpeg-master-latest-win64-gpl.zip",
            "ffmpeg-master-latest-linux64-gpl.tar.xz",
            "qjs-windows-x86_64.exe",
            "qjs-linux-x86_64",
            "qjs-darwin-arm64",
            "qjs-darwin-x86_64",
        ];
        for tool in all() {
            match &tool.source {
                ToolSource::GitHub {
                    release_api,
                    asset_name,
                } => {
                    assert!(
                        release_api.ends_with("/releases/latest")
                            || release_api.ends_with("/releases/tags/latest"),
                        "{release_api}"
                    );
                    // Asset names carry no version or date, or they would expire.
                    assert!(allowed.contains(&asset_name.as_str()), "{asset_name}");
                }
                ToolSource::Redirect { latest_url } => {
                    assert!(latest_url.contains("/redirect/latest/"), "{latest_url}");
                    assert_eq!(tool.version_source, VersionSource::BuildId);
                }
            }
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
        for (tools, exe) in [
            (windows_x64_tools(), ".exe"),
            (linux_x64_tools(), ""),
            (macos_arm64_tools(), ""),
            (macos_x64_tools(), ""),
        ] {
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
        for tools in [
            windows_x64_tools(),
            linux_x64_tools(),
            macos_arm64_tools(),
            macos_x64_tools(),
        ] {
            let (qjs, ffmpeg) = tools.split_last().unwrap();
            assert_eq!(qjs.name, "qjs");
            assert_eq!(qjs.policy, UpdatePolicy::WhenVersionChanges);
            for part in ffmpeg {
                assert_eq!(part.name, "ffmpeg");
                assert_eq!(part.policy, UpdatePolicy::IfMissing);
            }
        }
    }

    #[test]
    fn macos_asks_the_site_for_the_builds_of_its_own_cpu() {
        let url = |tools: Vec<PlatformTool>, index: usize| match &tools[index].source {
            ToolSource::Redirect { latest_url } => latest_url.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            url(macos_arm64_tools(), 0),
            "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip"
        );
        assert_eq!(
            url(macos_x64_tools(), 1),
            "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffprobe.zip"
        );
    }
}
