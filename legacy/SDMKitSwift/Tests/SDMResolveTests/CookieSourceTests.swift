import Testing

@testable import SDMResolve

@Test func noneProducesNoArguments() {
    #expect(CookieSource.none.ytDlpArguments == [])
}

@Test func chromeProducesCookiesFromBrowserArgument() {
    #expect(CookieSource.chrome.ytDlpArguments == ["--cookies-from-browser", "chrome"])
}

@Test func everyBrowserCaseHasARawValue() {
    for source in CookieSource.allCases where source != .none {
        #expect(source.ytDlpArguments.count == 2)
    }
}
