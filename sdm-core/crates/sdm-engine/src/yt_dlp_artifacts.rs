use std::fs;
use std::path::{Path, PathBuf};

/// Finds the scratch files `yt-dlp` leaves in a folder when it is killed
/// mid-download (the wholesale path). A clean yt-dlp exit removes these
/// itself; a terminated process does not, so SDM sweeps them on pause /
/// failure / removal.
///
/// yt-dlp writes, alongside the output `Clip.mp4`:
///   - `Clip.mp4.ytdl`: resume metadata
///   - `Clip.mp4.part`: in-progress output (with `--no-part` the bare name is
///     used, but be defensive)
///   - `Clip.mp4-Frag0`, `Clip.mp4-Frag1`, …: individual HLS/DASH fragments
///     (also `Clip.mp4.part-FragN`, `Clip.mp4.fragN` across yt-dlp versions)
///   - `Clip.f137.mp4`, `Clip.f234.webm` and each of *their* `.ytdl` /
///     `-FragN` / `.part` siblings: the pre-merge per-stream files when a
///     format selector resolves to separate video + audio
///   - `Clip.temp.mp4`: the merge scratch file
pub enum YtDlpArtifacts {}

impl YtDlpArtifacts {
    /// Every file in `folder` that is a yt-dlp scratch file for the download
    /// whose final name is `output_filename`. Excludes `output_filename`
    /// itself — the caller decides whether the (possibly complete) output
    /// goes too. An unreadable folder yields nothing.
    pub fn scratch_files(folder: &Path, output_filename: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(folder) else {
            return Vec::new();
        };
        let stem = deleting_path_extension(output_filename);
        entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name != output_filename && Self::is_scratch(name, output_filename, stem))
            .map(|name| folder.join(name))
            .collect()
    }

    /// `scratch_files` plus `output_filename` itself.
    pub fn all_files(folder: &Path, output_filename: &str) -> Vec<PathBuf> {
        let mut files = Self::scratch_files(folder, output_filename);
        files.push(folder.join(output_filename));
        files
    }

    pub fn is_scratch(name: &str, output: &str, stem: &str) -> bool {
        if name == output {
            return false;
        }

        // Pre-merge per-stream files: `Clip.f137.mp4` and every one of their
        // own siblings (`Clip.f137.mp4.ytdl`, `Clip.f137.mp4-Frag3`, …).
        // Guard on `.f<digit>` so an unrelated `Clip.final.mp4` is not swept.
        let format_prefix = format!("{stem}.f");
        if let Some(after) = name.strip_prefix(format_prefix.as_str())
            && after.chars().next().is_some_and(char::is_numeric)
        {
            return true;
        }

        // Merge scratch: `Clip.temp.mp4`.
        if name.starts_with(format!("{stem}.temp.").as_str()) {
            return true;
        }

        // Direct siblings of the output, whatever the separator yt-dlp used:
        // `Clip.mp4.ytdl`, `Clip.mp4.part`, `Clip.mp4.part-Frag7`,
        // `Clip.mp4-Frag412`, `Clip.mp4.frag0`, `Clip.mp4.temp`.
        let Some(rest) = name.strip_prefix(output) else {
            return false;
        };
        let rest = rest.to_lowercase();
        [".ytdl", ".part", ".frag", "-frag", ".temp", ".ytdlp"]
            .iter()
            .any(|prefix| rest.starts_with(prefix))
    }
}

/// `(name as NSString).deletingPathExtension` for a bare file name: drops the
/// text after the last dot when there is a non-empty stem before it and at
/// least one character after.
fn deleting_path_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => &name[..index],
        _ => name,
    }
}
