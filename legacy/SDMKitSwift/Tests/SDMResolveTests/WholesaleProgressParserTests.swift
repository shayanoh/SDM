import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

@Test func parsesByteProgress() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1048576|10485760|NA|NA|NA")
    #expect(p?.downloadedBytes == 1_048_576)
    #expect(p?.totalBytes == 10_485_760)
    #expect(p?.phase == .downloading)
}

@Test func promotesEstimateWhenTotalMissing() {
    let p = WholesaleProgressParser.parse("sdm:downloading|500|NA|2000|NA|NA")
    #expect(p?.totalBytes == 2000)
}

@Test func parsesFloatFormattedEstimate() {
    // yt-dlp emits total_bytes_estimate as a float for HLS/DASH downloads.
    let p = WholesaleProgressParser.parse(
        "sdm:downloading|20420736|NA|13161769424.0|2|1318")
    #expect(p?.downloadedBytes == 20_420_736)
    #expect(p?.totalBytes == 13_161_769_424)
}

@Test func derivesFractionFromFragmentIndexAndCount() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1000|NA|NA|659|1318")
    #expect(p?.fraction == 0.5)
    // A fragment count means yt-dlp's native (resumable) downloader is in use.
    #expect(p?.isFragmented == true)
}

@Test func fragmentFractionIsNilWithoutFragmentInfo() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1000|4000|NA|NA|NA")
    #expect(p?.fraction == nil)
    #expect(p?.totalBytes == 4000)
    // No fragment count — not (yet) known to be the resumable downloader.
    #expect(p?.isFragmented == false)
}

@Test func handlesNoneAndCommaGroupedNumbers() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1,048,576|None|None|None|None")
    #expect(p?.downloadedBytes == 1_048_576)
    #expect(p?.totalBytes == nil)
    #expect(p?.fraction == nil)
}

@Test func recognizesPostProcessing() {
    #expect(
        WholesaleProgressParser.parse("[Merger] Merging formats into \"x.mp4\"")?.phase
            == .postProcessing)
    #expect(
        WholesaleProgressParser.parse(
            "[FixupM3u8] Fixing MPEG-TS in MP4 container of \"x.mp4\"")?.phase == .postProcessing)
}

@Test func ignoresUnrelatedLines() {
    #expect(WholesaleProgressParser.parse("[youtube] Extracting URL: https://x") == nil)
    #expect(WholesaleProgressParser.parse("") == nil)
    #expect(WholesaleProgressParser.parse("sdm:downloading|only|three") == nil)
}
