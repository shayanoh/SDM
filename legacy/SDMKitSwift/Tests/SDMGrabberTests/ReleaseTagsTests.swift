import Testing

@testable import SDMGrabber

@Test(
    arguments: [
        (
            "The.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GRP.mkv",
            "1080p · WEB-DL · H.264 · DDP · 5.1"
        ),
        (
            "Movie.Name.2024.2160p.UHD.BluRay.REMUX.HDR10.DV.TrueHD.Atmos.7.1.HEVC-FraMeSToR.mkv",
            "2160p · BluRay · REMUX · HDR10 · DV · HEVC · TrueHD · Atmos · 7.1"
        ),
        (
            "Some Movie (2019) [1080p] [WEBRip] [x265] [10bit] [AAC 5.1].mp4",
            "1080p · WEBRip · 10bit · x265 · AAC · 5.1"
        ),
        (
            "Series.S02E05.720p.HDTV.x264-KILLERS.mkv",
            "720p · HDTV · x264"
        ),
        (
            "Film.2021.1080p.AMZN.WEB-DL.DD+5.1.H.264-TEPES.mkv",
            "1080p · WEB-DL · AMZN · H.264 · DDP · 5.1"
        ),
        (
            "Documentary.2020.PROPER.REPACK.1080p.BluRay.x264-GROUP.mkv",
            "1080p · BluRay · x264 · PROPER · REPACK"
        ),
    ]
)
func extractsExpectedTags(_ filename: String, _ expected: String) {
    #expect(ReleaseTags.extract(from: filename) == expected)
}

@Test func returnsNilForOrdinaryFilenames() {
    #expect(ReleaseTags.extract(from: "invoice_2024.pdf") == nil)
    #expect(ReleaseTags.extract(from: "vacation-photos.zip") == nil)
    #expect(ReleaseTags.extract(from: "report final v3.docx") == nil)
    #expect(ReleaseTags.extract(from: "") == nil)
}

@Test func doesNotFalsePositiveOnEmbeddedDigits() {
    // "264" inside "S01E264" must not read as H.264; "dd" inside "add" must not
    // read as Dolby Digital.
    #expect(ReleaseTags.extract(from: "add.on.pack.S01E264.mp4") == nil)
}

@Test func matchesRegardlessOfSeparatorStyle() {
    #expect(
        ReleaseTags.extract(from: "Movie 2024 1080p BluRay x265.mkv")
            == "1080p · BluRay · x265")
    #expect(
        ReleaseTags.extract(from: "Movie_2024_1080p_BluRay_x265.mkv")
            == "1080p · BluRay · x265")
}
