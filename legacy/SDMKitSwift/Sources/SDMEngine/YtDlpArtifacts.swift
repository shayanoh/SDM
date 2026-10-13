import Foundation

/// Finds the scratch files `yt-dlp` leaves in a folder when it is killed
/// mid-download (the wholesale path). A clean yt-dlp exit removes these
/// itself; a terminated process does not, so SDM sweeps them on pause /
/// failure / removal.
///
/// yt-dlp writes, alongside the output `Clip.mp4`:
///   - `Clip.mp4.ytdl`               resume metadata
///   - `Clip.mp4.part`              in-progress output (with `--no-part` the
///                                  bare name is used, but be defensive)
///   - `Clip.mp4-Frag0`, `Clip.mp4-Frag1`, …   individual HLS/DASH fragments
///     (also `Clip.mp4.part-FragN`, `Clip.mp4.fragN` across yt-dlp versions)
///   - `Clip.f137.mp4`, `Clip.f234.webm` and each of *their* `.ytdl` /
///     `-FragN` / `.part` siblings — the pre-merge per-stream files when a
///     format selector resolves to separate video + audio
///   - `Clip.temp.mp4`               the merge scratch file
enum YtDlpArtifacts {
    /// Every file in `folder` that is a yt-dlp scratch file for the download
    /// whose final name is `outputFilename`. Excludes `outputFilename`
    /// itself — the caller decides whether the (possibly complete) output
    /// goes too.
    static func scratchFiles(in folder: URL, outputFilename: String) -> [URL] {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: folder.path) else { return [] }
        let stem = (outputFilename as NSString).deletingPathExtension
        return
            names
            .filter { $0 != outputFilename && isScratch($0, output: outputFilename, stem: stem) }
            .map { folder.appendingPathComponent($0) }
    }

    /// `scratchFiles` plus `outputFilename` itself.
    static func allFiles(in folder: URL, outputFilename: String) -> [URL] {
        scratchFiles(in: folder, outputFilename: outputFilename)
            + [folder.appendingPathComponent(outputFilename)]
    }

    static func isScratch(_ name: String, output: String, stem: String) -> Bool {
        if name == output { return false }

        // Pre-merge per-stream files: `Clip.f137.mp4` and every one of their
        // own siblings (`Clip.f137.mp4.ytdl`, `Clip.f137.mp4-Frag3`, …).
        // Guard on `.f<digit>` so an unrelated `Clip.final.mp4` is not swept.
        let formatPrefix = stem + ".f"
        if name.hasPrefix(formatPrefix),
            let next = name.dropFirst(formatPrefix.count).first, next.isNumber
        {
            return true
        }

        // Merge scratch: `Clip.temp.mp4`.
        if name.hasPrefix(stem + ".temp.") { return true }

        // Direct siblings of the output, whatever the separator yt-dlp used:
        // `Clip.mp4.ytdl`, `Clip.mp4.part`, `Clip.mp4.part-Frag7`,
        // `Clip.mp4-Frag412`, `Clip.mp4.frag0`, `Clip.mp4.temp`.
        guard name.hasPrefix(output) else { return false }
        let rest = name.dropFirst(output.count).lowercased()
        return rest.hasPrefix(".ytdl") || rest.hasPrefix(".part") || rest.hasPrefix(".frag")
            || rest.hasPrefix("-frag") || rest.hasPrefix(".temp") || rest.hasPrefix(".ytdlp")
    }
}
