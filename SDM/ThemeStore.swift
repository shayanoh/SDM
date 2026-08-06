import Observation
import SDMCore
import SwiftUI

/// Selection and live resolution for spec §10.1's theme system, mirroring
/// `EngineController`'s shape.
@MainActor
@Observable
final class ThemeStore {
    /// Sentinel `selectedID` meaning "follow the system appearance" rather
    /// than a fixed theme.
    static let systemSelectionID = "system"

    private static let key = "sdm.selectedThemeID"

    let catalog: [Theme]
    var selectedID: String {
        didSet {
            guard selectedID != oldValue else { return }
            UserDefaults.standard.set(selectedID, forKey: Self.key)
        }
    }

    init(catalog: [Theme] = ThemeCatalog.builtInThemes()) {
        self.catalog = catalog
        selectedID =
            UserDefaults.standard.string(forKey: Self.key) ?? Self.systemSelectionID
    }

    /// The theme actually in effect right now. "System" resolves against
    /// the caller's live `ColorScheme` rather than anything this class
    /// tracks itself.
    func resolved(for colorScheme: ColorScheme) -> Theme {
        let id =
            selectedID == Self.systemSelectionID
            ? (colorScheme == .dark ? "dark" : "light")
            : selectedID
        return catalog.first { $0.id == id } ?? catalog.first { $0.id == "light" } ?? fallback
    }

    private var fallback: Theme {
        catalog.first
            ?? Theme(
                id: "fallback", name: "Fallback", isDark: false, source: "SDM",
                surfacePrimary: "#FFFFFF", surfaceSecondary: "#F0F0F0", surfaceTertiary: "#E0E0E0",
                textPrimary: "#000000", textSecondary: "#444444", textTertiary: "#666666",
                accent: "#007AFF", border: "#CCCCCC", statusOnline: "#34C759",
                statusFaulty: "#FF9500", statusOffline: "#8E8E93", statusFailed: "#FF3B30",
                progressFill: "#007AFF", completedSegmentFill: "#34C759",
                activeHeadTint: "#32ADE6", graphStroke: "#007AFF", graphAverageStroke: "#5E5CE6",
                sidebarBackground: "#F0F0F0", selectionTint: "#007AFF")
    }
}
