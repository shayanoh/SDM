import Foundation

/// One of spec §10.1's ~20 semantic roles, resolved to a color per theme.
/// Views read roles, never literal colors, so a new theme is a new JSON
/// file with no view code touched. Colors are `#RRGGBB` hex strings rather
/// than a platform color type, keeping this pure and dependency-free like
/// every other `SDMCore` model — `Theme`'s app-layer counterpart bridges
/// each field to `SwiftUI.Color`.
///
/// Text-tier colors (`textPrimary`/`textSecondary`/`textTertiary`) are
/// deliberately shared across every dark-appearance theme, and separately
/// across every light-appearance theme, rather than one independent set per
/// palette — see `ThemeCatalog`'s bundled JSON for the values and Task 9's
/// WCAG-AA contrast test for why: a palette's own "muted"/"comment" color is
/// often tuned for a code editor's de-emphasis, not spec §10.1's AA gate.
public struct Theme: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var isDark: Bool
    /// Attribution for this theme's core palette, e.g. "Nord (MIT)" or
    /// "SDM" for the four built-ins that are not sourced from anywhere
    /// else. Spec §10.1: "each carries its license attribution."
    public var source: String

    public var surfacePrimary: String
    public var surfaceSecondary: String
    public var surfaceTertiary: String
    public var textPrimary: String
    public var textSecondary: String
    public var textTertiary: String
    public var accent: String
    public var border: String
    public var statusOnline: String
    public var statusFaulty: String
    public var statusOffline: String
    public var statusFailed: String
    public var progressFill: String
    public var completedSegmentFill: String
    public var activeHeadTint: String
    public var graphStroke: String
    public var graphAverageStroke: String
    public var sidebarBackground: String
    public var selectionTint: String

    public init(
        id: String,
        name: String,
        isDark: Bool,
        source: String,
        surfacePrimary: String,
        surfaceSecondary: String,
        surfaceTertiary: String,
        textPrimary: String,
        textSecondary: String,
        textTertiary: String,
        accent: String,
        border: String,
        statusOnline: String,
        statusFaulty: String,
        statusOffline: String,
        statusFailed: String,
        progressFill: String,
        completedSegmentFill: String,
        activeHeadTint: String,
        graphStroke: String,
        graphAverageStroke: String,
        sidebarBackground: String,
        selectionTint: String
    ) {
        self.id = id
        self.name = name
        self.isDark = isDark
        self.source = source
        self.surfacePrimary = surfacePrimary
        self.surfaceSecondary = surfaceSecondary
        self.surfaceTertiary = surfaceTertiary
        self.textPrimary = textPrimary
        self.textSecondary = textSecondary
        self.textTertiary = textTertiary
        self.accent = accent
        self.border = border
        self.statusOnline = statusOnline
        self.statusFaulty = statusFaulty
        self.statusOffline = statusOffline
        self.statusFailed = statusFailed
        self.progressFill = progressFill
        self.completedSegmentFill = completedSegmentFill
        self.activeHeadTint = activeHeadTint
        self.graphStroke = graphStroke
        self.graphAverageStroke = graphAverageStroke
        self.sidebarBackground = sidebarBackground
        self.selectionTint = selectionTint
    }
}
