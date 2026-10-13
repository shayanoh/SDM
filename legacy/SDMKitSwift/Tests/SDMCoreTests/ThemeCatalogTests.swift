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

@Test func thereAreSixteenBuiltInThemes() {
    #expect(ThemeCatalog.builtInThemes().count == 16)
}

/// Spec §10.1: "A test asserts WCAG AA contrast for every text-on-surface
/// pair in every bundled theme, so an attractive palette cannot ship
/// unreadable secondary text."
@Test func everyBuiltInThemePassesWCAGAAForAllTextSurfacePairs() {
    for theme in ThemeCatalog.builtInThemes() {
        let textColors = [theme.textPrimary, theme.textSecondary, theme.textTertiary]
        let surfaceColors = [theme.surfacePrimary, theme.surfaceSecondary, theme.surfaceTertiary]
        for text in textColors {
            for surface in surfaceColors {
                #expect(
                    ContrastRatio.passesAA(text, surface),
                    "\(theme.id): \(text) on \(surface) is below WCAG AA (ratio \(ContrastRatio.between(text, surface)))"
                )
            }
        }
    }
}
