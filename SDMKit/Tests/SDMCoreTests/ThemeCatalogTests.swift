import Testing

@testable import SDMCore

@Test func builtInThemesIncludesLightAndDark() {
    let themes = ThemeCatalog.builtInThemes()
    #expect(themes.contains { $0.id == "light" })
    #expect(themes.contains { $0.id == "dark" })
}

@Test func everyBuiltInThemeHasAUniqueID() {
    let ids = ThemeCatalog.builtInThemes().map(\.id)
    #expect(Set(ids).count == ids.count)
}

@Test func lightThemeIsNotDarkAndDarkThemeIsDark() {
    let themes = ThemeCatalog.builtInThemes()
    #expect(themes.first { $0.id == "light" }?.isDark == false)
    #expect(themes.first { $0.id == "dark" }?.isDark == true)
}
