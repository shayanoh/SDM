import SwiftUI

/// Spec §9.6: one `Canvas` path, no axes, no legend, y-scaled to its own
/// max. Takes its stroke color explicitly rather than reading
/// `@Environment(ThemeStore.self)` itself, since it is used inside `Canvas`
/// contexts where that plumbing would be awkward for no benefit — every
/// caller already has a `Theme` in scope.
struct Sparkline: View {
    let samples: [Double]
    let color: Color

    var body: some View {
        Canvas { context, size in
            guard size.width > 0 else { return }
            let windowed = Self.windowed(samples, to: Int(size.width.rounded()))
            guard windowed.count > 1, let maxValue = windowed.max(), maxValue > 0 else { return }
            var path = Path()
            let stepX = size.width / CGFloat(windowed.count - 1)
            for (index, value) in windowed.enumerated() {
                let x = CGFloat(index) * stepX
                let y = size.height - CGFloat(value / maxValue) * size.height
                if index == 0 {
                    path.move(to: CGPoint(x: x, y: y))
                } else {
                    path.addLine(to: CGPoint(x: x, y: y))
                }
            }
            context.stroke(path, with: .color(color), lineWidth: 1)
        }
    }

    /// Caps the drawn series at one sample per point of width, taking the
    /// most recent `width` samples, and left-pads with zeros when there
    /// isn't `width` samples yet so the sparkline doesn't rescale/stretch
    /// while it's still filling.
    private static func windowed(_ samples: [Double], to width: Int) -> [Double] {
        guard width > 0 else { return [] }
        if samples.count >= width {
            return Array(samples.suffix(width))
        }
        return Array(repeating: 0, count: width - samples.count) + samples
    }
}
