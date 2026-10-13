use sdm_grabber::ReleaseTags;

#[test]
fn extracts_expected_tags() {
    let cases = [
        (
            "The.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GRP.mkv",
            "1080p · WEB-DL · H.264 · DDP · 5.1",
        ),
        (
            "Movie.Name.2024.2160p.UHD.BluRay.REMUX.HDR10.DV.TrueHD.Atmos.7.1.HEVC-FraMeSToR.mkv",
            "2160p · BluRay · REMUX · HDR10 · DV · HEVC · TrueHD · Atmos · 7.1",
        ),
        (
            "Some Movie (2019) [1080p] [WEBRip] [x265] [10bit] [AAC 5.1].mp4",
            "1080p · WEBRip · 10bit · x265 · AAC · 5.1",
        ),
        (
            "Series.S02E05.720p.HDTV.x264-KILLERS.mkv",
            "720p · HDTV · x264",
        ),
        (
            "Film.2021.1080p.AMZN.WEB-DL.DD+5.1.H.264-TEPES.mkv",
            "1080p · WEB-DL · AMZN · H.264 · DDP · 5.1",
        ),
        (
            "Documentary.2020.PROPER.REPACK.1080p.BluRay.x264-GROUP.mkv",
            "1080p · BluRay · x264 · PROPER · REPACK",
        ),
    ];
    for (filename, expected) in cases {
        assert_eq!(
            ReleaseTags::extract(filename).as_deref(),
            Some(expected),
            "{filename}"
        );
    }
}

#[test]
fn returns_nil_for_ordinary_filenames() {
    assert_eq!(ReleaseTags::extract("invoice_2024.pdf"), None);
    assert_eq!(ReleaseTags::extract("vacation-photos.zip"), None);
    assert_eq!(ReleaseTags::extract("report final v3.docx"), None);
    assert_eq!(ReleaseTags::extract(""), None);
}

#[test]
fn does_not_false_positive_on_embedded_digits() {
    // "264" inside "S01E264" must not read as H.264; "dd" inside "add" must not
    // read as Dolby Digital.
    assert_eq!(ReleaseTags::extract("add.on.pack.S01E264.mp4"), None);
}

#[test]
fn matches_regardless_of_separator_style() {
    assert_eq!(
        ReleaseTags::extract("Movie 2024 1080p BluRay x265.mkv").as_deref(),
        Some("1080p · BluRay · x265")
    );
    assert_eq!(
        ReleaseTags::extract("Movie_2024_1080p_BluRay_x265.mkv").as_deref(),
        Some("1080p · BluRay · x265")
    );
}
