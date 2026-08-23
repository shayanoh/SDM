import AppKit
import Observation
import SwiftUI

/// Rasterizes `MenuBarRingIcon` into the status item's `NSImage`, but only
/// when the visible result would actually change. Regenerating on every
/// heartbeat tick regardless — the previous behavior — paid for a full
/// `ImageRenderer` pass and an `NSStatusBarButtonCell` redraw for a ring
/// that, most ticks, hadn't moved by even a rendered pixel.
@MainActor
@Observable
final class MenuBarIconController {
    private(set) var image: NSImage
    private var cachedFraction: Double
    private var cachedDrawCircle: Bool

    private static let renderScale: CGFloat = 2
    private static let iconPointWidth: CGFloat = 14
    /// `MenuBarRingIcon` fills a 14pt-wide frame at `renderScale`, so a
    /// fraction change smaller than one rendered pixel can't move the ring
    /// at all — anything under this delta is visually identical to the
    /// cached image.
    private static let minimumFractionDelta = 1.0 / (iconPointWidth * renderScale)

    init() {
        cachedFraction = 0
        cachedDrawCircle = false
        image = Self.render(fraction: 0, drawCircle: false)
    }

    func update(fraction: Double, drawCircle: Bool) {
        guard
            drawCircle != cachedDrawCircle
                || abs(fraction - cachedFraction) >= Self.minimumFractionDelta
        else { return }
        cachedFraction = fraction
        cachedDrawCircle = drawCircle
        image = Self.render(fraction: fraction, drawCircle: drawCircle)
    }

    /// `MenuBarExtra`'s custom label view ignores `.frame`/sizing modifiers
    /// on live SwiftUI content — the status item falls back to the image's
    /// native pixel size, rendering oversized and cropped. Rasterizing to a
    /// fixed-size `NSImage` sidesteps that: the label only ever sees a
    /// plain bitmap at the exact size asked for.
    private static func render(fraction: Double, drawCircle: Bool) -> NSImage {
        let renderer = ImageRenderer(
            content: MenuBarRingIcon(fraction: fraction, drawCircle: drawCircle))
        renderer.scale = renderScale
        guard let rendered = renderer.nsImage else { return NSImage() }
        rendered.isTemplate = true
        return rendered
    }
}
