import SwiftUI

/// Spec §9.6: one stroked path, no axes, no legend, y-scaled to its own
/// max. Takes its stroke color explicitly rather than reading
/// `@Environment(ThemeStore.self)` itself — every caller already has a
/// `Theme` in scope.
///
/// A stroked `Shape` rather than the `Canvas` it used to be: the same path,
/// without a drawing surface of its own for every row and package header.
/// `.clipped()` keeps what a `Canvas` did implicitly — a line running along
/// the top or bottom edge shows only the half of its width inside the
/// frame.
struct Sparkline: View {
    let samples: [Double]
    let color: Color

    var body: some View {
        SparklineShape(samples: samples)
            .stroke(color, lineWidth: 1)
            .clipped()
    }
}

private struct SparklineShape: Shape {
    let samples: [Double]

    nonisolated func path(in rect: CGRect) -> Path {
        var path = Path()
        guard rect.width > 0 else { return path }
        let windowed = Self.windowed(samples, to: Int(rect.width.rounded()))
        guard windowed.count > 1, let maxValue = windowed.max(), maxValue > 0 else { return path }
        let stepX = rect.width / CGFloat(windowed.count - 1)
        for (index, value) in windowed.enumerated() {
            let point = CGPoint(
                x: rect.minX + CGFloat(index) * stepX,
                y: rect.minY + rect.height - CGFloat(value / maxValue) * rect.height)
            if index == 0 {
                path.move(to: point)
            } else {
                path.addLine(to: point)
            }
        }
        return path
    }

    /// Caps the drawn series at one sample per point of width, taking the
    /// most recent `width` samples, and left-pads with zeros when there
    /// isn't `width` samples yet so the sparkline doesn't rescale/stretch
    /// while it's still filling.
    private nonisolated static func windowed(_ samples: [Double], to width: Int) -> [Double] {
        guard width > 0 else { return [] }
        if samples.count >= width {
            return Array(samples.suffix(width))
        }
        return Array(repeating: 0, count: width - samples.count) + samples
    }
}
