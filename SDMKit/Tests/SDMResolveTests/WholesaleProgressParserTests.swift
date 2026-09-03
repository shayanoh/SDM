import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

@Test func parsesByteProgress() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1048576|10485760|NA| 10.0%")
    #expect(p?.downloadedBytes == 1_048_576)
    #expect(p?.totalBytes == 10_485_760)
    #expect(p?.phase == .downloading)
}

@Test func promotesEstimateWhenTotalMissing() {
    let p = WholesaleProgressParser.parse("sdm:downloading|500|NA|2000| 25.0%")
    #expect(p?.totalBytes == 2000)
    #expect(p?.fraction == 0.25)
}

@Test func handlesNoneAndCommaGroupedNumbers() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1,048,576|None|None| 5.0%")
    #expect(p?.downloadedBytes == 1_048_576)
    #expect(p?.totalBytes == nil)
    #expect(p?.fraction == 0.05)
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
