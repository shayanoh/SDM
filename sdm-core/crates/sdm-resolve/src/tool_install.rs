//! Verifies a downloaded tool archive and installs its files into `bin/`.
//!
//! Nothing reaches `bin/` unless the whole download checked out: the SHA-256
//! is compared before the archive is opened, every wanted member is staged in
//! a temporary file first, and only then are the staged files renamed into
//! place. Members are looked up by their exact configured name, so a hostile
//! archive entry such as `../evil` is never written anywhere.

use std::fs::{self, File};
use std::io::{self, BufReader, Cursor};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::platform_tools::{ArchiveKind, PlatformTool, ToolMember};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolInstallError {
    #[error("checksumMismatch")]
    ChecksumMismatch,
    #[error("memberMissing({0})")]
    MemberMissing(String),
    #[error("archive({0})")]
    Archive(String),
    #[error("io({0})")]
    Io(String),
}

impl From<io::Error> for ToolInstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Verifies `bytes` against `sha256` (lowercase or uppercase hex), then writes
/// every member of `tool` into `bin_dir` (executable on Unix). Either all
/// members land or none do.
pub fn install_tool(
    tool: &PlatformTool,
    sha256: &str,
    bytes: &[u8],
    bin_dir: &Path,
) -> Result<(), ToolInstallError> {
    if hex::encode(Sha256::digest(bytes)) != sha256.to_ascii_lowercase() {
        return Err(ToolInstallError::ChecksumMismatch);
    }
    fs::create_dir_all(bin_dir)?;
    let mut staged = Staged::new(bin_dir);
    let result = stage_members(tool, bytes, bin_dir, &mut staged);
    match result {
        Ok(()) => staged.commit(),
        Err(error) => Err(error), // `staged` drops and removes its temporary files
    }
}

/// Temporary files for the members; removed on drop unless committed.
struct Staged {
    bin_dir: PathBuf,
    files: Vec<(PathBuf, PathBuf)>, // (temporary, final)
    committed: bool,
}

impl Staged {
    fn new(bin_dir: &Path) -> Self {
        Self {
            bin_dir: bin_dir.to_path_buf(),
            files: Vec::new(),
            committed: false,
        }
    }

    /// A fresh temporary file for `member`; the caller fills it.
    fn create(&mut self, member: &ToolMember) -> Result<File, ToolInstallError> {
        let file_name = Path::new(&member.file_name)
            .file_name()
            .ok_or_else(|| ToolInstallError::Archive("a member has no file name".into()))?;
        let temporary = self
            .bin_dir
            .join(format!(".{}.install", file_name.to_string_lossy()));
        let file = File::create(&temporary)?;
        self.files.push((temporary, self.bin_dir.join(file_name)));
        Ok(file)
    }

    fn commit(mut self) -> Result<(), ToolInstallError> {
        for (temporary, target) in &self.files {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(temporary, fs::Permissions::from_mode(0o755))?;
            }
            // Windows cannot rename over an existing file.
            if target.exists() {
                fs::remove_file(target)?;
            }
            fs::rename(temporary, target)?;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            for (temporary, _) in &self.files {
                let _ = fs::remove_file(temporary);
            }
        }
    }
}

fn stage_members(
    tool: &PlatformTool,
    bytes: &[u8],
    bin_dir: &Path,
    staged: &mut Staged,
) -> Result<(), ToolInstallError> {
    match tool.archive {
        ArchiveKind::Raw => {
            let member = tool
                .members
                .first()
                .ok_or_else(|| ToolInstallError::MemberMissing(tool.name.clone()))?;
            let mut file = staged.create(member)?;
            io::copy(&mut Cursor::new(bytes), &mut file)?;
            Ok(())
        }
        ArchiveKind::Zip => {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
            for member in &tool.members {
                let index = (0..archive.len())
                    .find(|&i| {
                        matches!(
                            archive.name_for_index(i),
                            Some(Ok(name)) if entry_matches(&name, &member.path_in_archive)
                        )
                    })
                    .ok_or_else(|| {
                        ToolInstallError::MemberMissing(member.path_in_archive.clone())
                    })?;
                let mut entry = archive
                    .by_index(index)
                    .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
                let mut file = staged.create(member)?;
                io::copy(&mut entry, &mut file)
                    .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
            }
            Ok(())
        }
        ArchiveKind::TarXz => {
            // Inflate to a temporary tar on disk: the archive is 150 MB and
            // its tar several times that, which should not sit in memory.
            let tar_path = bin_dir.join(".tools.install.tar");
            let _cleanup = RemoveOnDrop(tar_path.clone());
            {
                let mut tar_file = File::create(&tar_path)?;
                lzma_rs::xz_decompress(&mut BufReader::new(Cursor::new(bytes)), &mut tar_file)
                    .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
            }
            for member in &tool.members {
                let mut archive = tar::Archive::new(BufReader::new(File::open(&tar_path)?));
                let entries = archive
                    .entries()
                    .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
                let mut found = false;
                for entry in entries {
                    let mut entry = entry.map_err(|e| ToolInstallError::Archive(e.to_string()))?;
                    let is_wanted = entry
                        .path()
                        .map(|p| entry_matches(&p.to_string_lossy(), &member.path_in_archive))
                        .unwrap_or(false);
                    if is_wanted {
                        let mut file = staged.create(member)?;
                        io::copy(&mut entry, &mut file)
                            .map_err(|e| ToolInstallError::Archive(e.to_string()))?;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(ToolInstallError::MemberMissing(
                        member.path_in_archive.clone(),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Whether an archive entry is the wanted member: its path equals `wanted` or
/// ends with `/wanted` (the build's top folder has a different name each time).
/// An entry that climbs out (`..`) or is absolute never matches.
fn entry_matches(entry: &str, wanted: &str) -> bool {
    let entry = entry.replace('\\', "/");
    if entry.starts_with('/') || entry.split('/').any(|part| part == "..") {
        return false;
    }
    entry == wanted || entry.ends_with(&format!("/{wanted}"))
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tool(archive: ArchiveKind, _bytes: &[u8], members: &[(&str, &str)]) -> PlatformTool {
        PlatformTool {
            name: "ffmpeg".into(),
            release_api: "https://api.github.com/repos/o/r/releases/latest".into(),
            asset_name: "asset".into(),
            archive,
            members: members
                .iter()
                .map(|(a, b)| ToolMember::new(*a, *b))
                .collect(),
            policy: crate::platform_tools::UpdatePolicy::IfMissing,
            version_source: crate::platform_tools::VersionSource::Tag,
        }
    }

    fn sum(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            for (name, data) in entries {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        out.into_inner()
    }

    fn tar_xz_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (name, data) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, *data).unwrap();
            }
            builder.finish().unwrap();
        }
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut BufReader::new(Cursor::new(tar_bytes)), &mut xz).unwrap();
        xz
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn installs_members_from_zip() {
        let bytes = zip_bytes(&[
            ("d/bin/ffmpeg.exe", b"FF"),
            ("d/bin/ffprobe.exe", b"PR"),
            ("d/readme", b"x"),
        ]);
        let t = tool(
            ArchiveKind::Zip,
            &bytes,
            &[
                ("d/bin/ffmpeg.exe", "ffmpeg.exe"),
                ("d/bin/ffprobe.exe", "ffprobe.exe"),
            ],
        );
        let dir = tempfile::tempdir().unwrap();
        install_tool(&t, &sum(&bytes), &bytes, dir.path()).unwrap();
        assert_eq!(names(dir.path()), vec!["ffmpeg.exe", "ffprobe.exe"]);
        assert_eq!(fs::read(dir.path().join("ffmpeg.exe")).unwrap(), b"FF");
        assert_eq!(fs::read(dir.path().join("ffprobe.exe")).unwrap(), b"PR");
    }

    #[test]
    fn installs_members_from_tar_xz() {
        let bytes = tar_xz_bytes(&[
            ("d/bin/ffmpeg", b"FF"),
            ("d/bin/ffprobe", b"PR"),
            ("d/other", b"x"),
        ]);
        let t = tool(
            ArchiveKind::TarXz,
            &bytes,
            &[("d/bin/ffmpeg", "ffmpeg"), ("d/bin/ffprobe", "ffprobe")],
        );
        let dir = tempfile::tempdir().unwrap();
        install_tool(&t, &sum(&bytes), &bytes, dir.path()).unwrap();
        assert_eq!(names(dir.path()), vec!["ffmpeg", "ffprobe"]);
        assert_eq!(fs::read(dir.path().join("ffprobe")).unwrap(), b"PR");
    }

    #[test]
    fn raw_archive_writes_the_bytes() {
        let t = tool(ArchiveKind::Raw, b"QJS", &[("", "qjs")]);
        let dir = tempfile::tempdir().unwrap();
        install_tool(&t, &sum(b"QJS"), b"QJS", dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("qjs")).unwrap(), b"QJS");
    }

    #[test]
    fn checksum_mismatch_writes_nothing() {
        let t = tool(ArchiveKind::Raw, b"QJS", &[("", "qjs")]);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            install_tool(&t, &"0".repeat(64), b"QJS", dir.path()),
            Err(ToolInstallError::ChecksumMismatch)
        );
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn missing_member_writes_nothing() {
        let bytes = zip_bytes(&[("d/bin/ffmpeg.exe", b"FF")]);
        let t = tool(
            ArchiveKind::Zip,
            &bytes,
            &[
                ("d/bin/ffmpeg.exe", "ffmpeg.exe"),
                ("d/bin/ffprobe.exe", "ffprobe.exe"),
            ],
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            install_tool(&t, &sum(&bytes), &bytes, dir.path()),
            Err(ToolInstallError::MemberMissing("d/bin/ffprobe.exe".into()))
        );
        assert!(names(dir.path()).is_empty());
        let bytes = tar_xz_bytes(&[("d/bin/ffmpeg", b"FF")]);
        let t = tool(
            ArchiveKind::TarXz,
            &bytes,
            &[("d/bin/ffmpeg", "ffmpeg"), ("d/bin/ffprobe", "ffprobe")],
        );
        assert!(matches!(
            install_tool(&t, &sum(&bytes), &bytes, dir.path()),
            Err(ToolInstallError::MemberMissing(_))
        ));
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn truncated_archive_leaves_bin_clean() {
        let full = zip_bytes(&[("d/bin/ffmpeg.exe", &[7u8; 4096])]);
        let truncated = &full[..full.len() / 2];
        let t = tool(
            ArchiveKind::Zip,
            truncated,
            &[("d/bin/ffmpeg.exe", "ffmpeg.exe")],
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            install_tool(&t, &sum(truncated), truncated, dir.path()),
            Err(ToolInstallError::Archive(_))
        ));
        assert!(names(dir.path()).is_empty());
        let full = tar_xz_bytes(&[("d/bin/ffmpeg", &[7u8; 4096])]);
        let truncated = &full[..full.len() / 2];
        let t = tool(ArchiveKind::TarXz, truncated, &[("d/bin/ffmpeg", "ffmpeg")]);
        assert!(install_tool(&t, &sum(truncated), truncated, dir.path()).is_err());
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn path_traversal_member_is_never_written_outside_bin() {
        let bytes = zip_bytes(&[("../evil", b"X"), ("d/bin/ffmpeg.exe", b"FF")]);
        let t = tool(
            ArchiveKind::Zip,
            &bytes,
            &[("d/bin/ffmpeg.exe", "ffmpeg.exe")],
        );
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        install_tool(&t, &sum(&bytes), &bytes, &bin).unwrap();
        assert!(!root.path().join("evil").exists());
        assert_eq!(names(&bin), vec!["ffmpeg.exe"]);
        // A member whose configured destination name is a path keeps only its file name.
        let t = tool(ArchiveKind::Raw, b"Q", &[("", "../escape")]);
        install_tool(&t, &sum(b"Q"), b"Q", &bin).unwrap();
        assert!(!root.path().join("escape").exists());
        assert!(bin.join("escape").exists());
    }

    #[test]
    fn a_member_is_found_under_any_top_folder_and_never_through_dot_dot() {
        let bytes = zip_bytes(&[
            ("../bin/ffmpeg.exe", b"EVIL"),
            ("ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe", b"FF"),
            (
                "ffmpeg-master-latest-win64-gpl/doc/bin/ffmpeg.exe.txt",
                b"X",
            ),
        ]);
        let t = tool(
            ArchiveKind::Zip,
            &bytes,
            &[("bin/ffmpeg.exe", "ffmpeg.exe")],
        );
        let dir = tempfile::tempdir().unwrap();
        install_tool(&t, &sum(&bytes), &bytes, dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("ffmpeg.exe")).unwrap(), b"FF");

        let only_evil = zip_bytes(&[("../bin/ffmpeg.exe", b"EVIL")]);
        let t = tool(
            ArchiveKind::Zip,
            &only_evil,
            &[("bin/ffmpeg.exe", "ffmpeg.exe")],
        );
        let clean = tempfile::tempdir().unwrap();
        assert!(matches!(
            install_tool(&t, &sum(&only_evil), &only_evil, clean.path()),
            Err(ToolInstallError::MemberMissing(_))
        ));
        assert!(names(clean.path()).is_empty());
    }

    #[test]
    fn a_partial_name_is_not_a_match() {
        assert!(entry_matches("top/bin/ffmpeg", "bin/ffmpeg"));
        assert!(entry_matches("bin/ffmpeg", "bin/ffmpeg"));
        assert!(!entry_matches("top/xbin/ffmpeg", "bin/ffmpeg"));
        assert!(!entry_matches("/bin/ffmpeg", "bin/ffmpeg"));
        assert!(entry_matches("top\\bin\\ffmpeg", "bin/ffmpeg"));
    }

    #[cfg(unix)]
    #[test]
    fn executable_bit_is_set_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let t = tool(ArchiveKind::Raw, b"Q", &[("", "qjs")]);
        let dir = tempfile::tempdir().unwrap();
        install_tool(&t, &sum(b"Q"), b"Q", dir.path()).unwrap();
        let mode = fs::metadata(dir.path().join("qjs"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn installing_over_an_existing_file_replaces_it() {
        let t = tool(ArchiveKind::Raw, b"NEW", &[("", "qjs")]);
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("qjs"), b"OLD").unwrap();
        install_tool(&t, &sum(b"NEW"), b"NEW", dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("qjs")).unwrap(), b"NEW");
    }
}
