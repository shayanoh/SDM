import Testing

@testable import SDMCore

@Test func blackOnWhiteHasTheMaximumContrastRatio() {
    let ratio = ContrastRatio.between("#000000", "#FFFFFF")
    #expect(abs(ratio - 21.0) < 0.01)
}

@Test func identicalColorsHaveARatioOfOne() {
    let ratio = ContrastRatio.between("#808080", "#808080")
    #expect(abs(ratio - 1.0) < 0.01)
}

@Test func contrastRatioIsSymmetric() {
    #expect(
        ContrastRatio.between("#000000", "#FFFFFF") == ContrastRatio.between("#FFFFFF", "#000000"))
}

@Test func blackOnWhitePassesAA() {
    #expect(ContrastRatio.passesAA("#000000", "#FFFFFF"))
}

@Test func identicalGraysFailAA() {
    #expect(!ContrastRatio.passesAA("#808080", "#808080"))
}

@Test func hexWithoutALeadingHashIsAccepted() {
    #expect(
        ContrastRatio.between("000000", "FFFFFF") == ContrastRatio.between("#000000", "#FFFFFF"))
}
