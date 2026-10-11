import Testing

@testable import SDMRust

@Test func coreVersionCrossesTheBoundary() {
    let version = coreVersion()
    #expect(!version.version.isEmpty)
    #expect(version.crates.contains("sdm-engine"))
}

@Test func asyncCallsCrossTheBoundary() async {
    #expect(await coreEchoAfterYield(text: "hello") == "hello")
}
