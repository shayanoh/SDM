import SDMCore
import SwiftUI

extension Color {
    /// Decodes a `#RRGGBB` hex string from a `Theme` role into a `Color`.
    /// Every bundled theme is a fixture this project controls, so a
    /// malformed hex is a bug to catch immediately via `precondition`, not a
    /// runtime condition to recover from.
    init(hex: String) {
        var cleaned = hex.trimmingCharacters(in: .whitespaces)
        if cleaned.hasPrefix("#") { cleaned.removeFirst() }
        precondition(cleaned.count == 6, "expected a #RRGGBB hex color, got \(hex)")
        var value: UInt64 = 0
        Scanner(string: cleaned).scanHexInt64(&value)
        self.init(
            red: Double((value & 0xFF0000) >> 16) / 255,
            green: Double((value & 0x00FF00) >> 8) / 255,
            blue: Double(value & 0x0000FF) / 255
        )
    }
}

extension Theme {
    var surfacePrimaryColor: Color { Color(hex: surfacePrimary) }
    var surfaceSecondaryColor: Color { Color(hex: surfaceSecondary) }
    var surfaceTertiaryColor: Color { Color(hex: surfaceTertiary) }
    var textPrimaryColor: Color { Color(hex: textPrimary) }
    var textSecondaryColor: Color { Color(hex: textSecondary) }
    var textTertiaryColor: Color { Color(hex: textTertiary) }
    var accentColor: Color { Color(hex: accent) }
    var borderColor: Color { Color(hex: border) }
    var onlineColor: Color { Color(hex: statusOnline) }
    var faultyColor: Color { Color(hex: statusFaulty) }
    var offlineColor: Color { Color(hex: statusOffline) }
    var failedColor: Color { Color(hex: statusFailed) }
    var progressFillColor: Color { Color(hex: progressFill) }
    var completedSegmentFillColor: Color { Color(hex: completedSegmentFill) }
    var activeHeadTintColor: Color { Color(hex: activeHeadTint) }
    var graphStrokeColor: Color { Color(hex: graphStroke) }
    var graphAverageStrokeColor: Color { Color(hex: graphAverageStroke) }
    var sidebarBackgroundColor: Color { Color(hex: sidebarBackground) }
    var selectionTintColor: Color { Color(hex: selectionTint) }
}
