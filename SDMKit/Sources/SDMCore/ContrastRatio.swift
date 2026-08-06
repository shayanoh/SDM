import Foundation

/// WCAG 2.1 contrast-ratio math over sRGB hex colors, used to gate every
/// bundled theme's text-on-surface pairs at AA level (spec §10.1). Pure so
/// it is testable without touching AppKit/SwiftUI color types.
public enum ContrastRatio {
    /// The WCAG contrast ratio between two `#RRGGBB` hex colors, from `1`
    /// (identical) to `21` (black on white).
    public static func between(_ hexA: String, _ hexB: String) -> Double {
        let luminanceA = relativeLuminance(of: hexA)
        let luminanceB = relativeLuminance(of: hexB)
        let lighter = Swift.max(luminanceA, luminanceB)
        let darker = Swift.min(luminanceA, luminanceB)
        return (lighter + 0.05) / (darker + 0.05)
    }

    /// WCAG AA for normal text: a contrast ratio of at least 4.5:1.
    public static func passesAA(_ hexA: String, _ hexB: String) -> Bool {
        between(hexA, hexB) >= 4.5
    }

    private static func relativeLuminance(of hex: String) -> Double {
        let (r, g, b) = components(of: hex)
        func linearize(_ channel: Double) -> Double {
            channel <= 0.03928 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
    }

    private static func components(of hex: String) -> (Double, Double, Double) {
        var cleaned = hex.trimmingCharacters(in: .whitespaces)
        if cleaned.hasPrefix("#") { cleaned.removeFirst() }
        precondition(cleaned.count == 6, "expected a #RRGGBB hex color, got \(hex)")
        let scanner = Scanner(string: cleaned)
        var value: UInt64 = 0
        scanner.scanHexInt64(&value)
        let r = Double((value & 0xFF0000) >> 16) / 255
        let g = Double((value & 0x00FF00) >> 8) / 255
        let b = Double(value & 0x0000FF) / 255
        return (r, g, b)
    }
}
