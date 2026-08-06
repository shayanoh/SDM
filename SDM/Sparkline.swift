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
            guard samples.count > 1, let maxValue = samples.max(), maxValue > 0 else { return }
            var path = Path()
            let stepX = size.width / CGFloat(samples.count - 1)
            for (index, value) in samples.enumerated() {
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
}
