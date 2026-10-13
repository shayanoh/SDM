use std::collections::HashSet;
use std::fs;

use sdm_engine::YtDlpArtifacts;

#[test]
fn identifies_yt_dlp_scratch_siblings() {
    let out = "Clip [abc123].mp4";
    let stem = "Clip [abc123]";
    let scratch = [
        "Clip [abc123].mp4.ytdl",
        "Clip [abc123].mp4.part",
        "Clip [abc123].mp4.part-Frag17",
        "Clip [abc123].mp4-Frag25",
        "Clip [abc123].mp4-Frag412",
        "Clip [abc123].mp4.frag0",
        "Clip [abc123].f137.mp4",
        "Clip [abc123].f137.mp4.ytdl",
        "Clip [abc123].f234.webm-Frag3",
        "Clip [abc123].temp.mp4",
    ];
    for name in scratch {
        assert!(YtDlpArtifacts::is_scratch(name, out, stem), "{name}");
    }
}

#[test]
fn does_not_sweep_unrelated_files() {
    let out = "Clip [abc123].mp4";
    let stem = "Clip [abc123]";
    for name in [
        "Clip [abc123].mp4",       // the output itself
        "Clip [abc123].final.mp4", // not a .f<digit> stream file
        "Other Video [xyz].mp4",
        "Clip [abc123].en.srt",
        "poster.jpg",
    ] {
        assert!(!YtDlpArtifacts::is_scratch(name, out, stem), "{name}");
    }
}

#[test]
fn scratch_files_scans_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "V.mp4",
        "V.mp4.ytdl",
        "V.mp4-Frag9",
        "V.f137.mp4",
        "V.f137.mp4.part",
        "unrelated.txt",
    ] {
        fs::write(dir.path().join(name), b"").unwrap();
    }
    let found: HashSet<String> = YtDlpArtifacts::scratch_files(dir.path(), "V.mp4")
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let expected: HashSet<String> = ["V.mp4.ytdl", "V.mp4-Frag9", "V.f137.mp4", "V.f137.mp4.part"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(found, expected);
}
