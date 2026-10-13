import Foundation
import Testing

@testable import SDMCore

@Test func themeRoundTripsThroughJSON() throws {
    let theme = Theme(
        id: "test", name: "Test", isDark: true, source: "SDM",
        surfacePrimary: "#111111", surfaceSecondary: "#222222", surfaceTertiary: "#333333",
        textPrimary: "#FFFFFF", textSecondary: "#DDDDDD", textTertiary: "#BBBBBB",
        accent: "#4488FF", border: "#444444",
        statusOnline: "#33CC66", statusFaulty: "#FFAA33", statusOffline: "#888888",
        statusFailed: "#FF4444",
        progressFill: "#4488FF", completedSegmentFill: "#33CC66", activeHeadTint: "#66CCFF",
        graphStroke: "#4488FF", graphAverageStroke: "#AA66FF",
        sidebarBackground: "#0A0A0A", selectionTint: "#4488FF"
    )
    let data = try JSONEncoder().encode(theme)
    let decoded = try JSONDecoder().decode(Theme.self, from: data)
    #expect(decoded == theme)
}
