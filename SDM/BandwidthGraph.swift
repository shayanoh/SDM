import SwiftUI

/// Hand-rolled in place of Swift Charts. Spec §9.6 originally reserved Charts
/// for this one graph, reasoning that per-row sparklines (a `Canvas` each)
/// were the cost Charts wasn't worth paying at list-row scale, but that a
/// single global graph would earn it. Profiling showed otherwise: even after
/// fixing the bugs that made it redraw every tick regardless of activity,
/// laying out up to 1500 points across two marks (area + line) through
/// Charts' AttributeGraph-backed mark diffing measurably outweighs drawing
/// the same two paths directly. A `Canvas` draws both in one immediate-mode
/// pass with no per-point view identity, matching what the sparklines
/// already do.
struct BandwidthGraph: View {
    let history: [Double]
    let strokeColor: Color
    let averageStrokeColor: Color

    var body: some View {
        Canvas { context, size in
            guard history.count > 1, size.width > 0, size.height > 0 else { return }
            let maxValue = max(history.max() ?? 0, 1)
            let stepX = size.width / CGFloat(history.count - 1)

            func point(_ index: Int, _ value: Double) -> CGPoint {
                CGPoint(
                    x: CGFloat(index) * stepX,
                    y: size.height - CGFloat(value / maxValue) * size.height)
            }

            var areaPath = Path()
            areaPath.move(to: CGPoint(x: 0, y: size.height))
            for (index, value) in history.enumerated() {
                areaPath.addLine(to: point(index, value))
            }
            areaPath.addLine(to: CGPoint(x: size.width, y: size.height))
            areaPath.closeSubpath()
            context.fill(areaPath, with: .color(strokeColor.opacity(0.25)))

            let averages = runningAverage
            var linePath = Path()
            linePath.move(to: point(0, averages[0]))
            for index in averages.indices.dropFirst() {
                linePath.addLine(to: point(index, averages[index]))
            }
            context.stroke(linePath, with: .color(averageStrokeColor), lineWidth: 1.5)
        }
    }

    private var runningAverage: [Double] {
        guard !history.isEmpty else { return [] }
        var result: [Double] = []
        var sum = 0.0
        for (index, value) in history.enumerated() {
            sum += value
            result.append(sum / Double(index + 1))
        }
        return result
    }
}
