import SDMCore
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
            guard size.width > 0, size.height > 0 else { return }
            let windowed = Self.windowed(history, to: Int(size.width.rounded()))
            guard windowed.count > 1 else { return }
            let maxValue = max(windowed.max() ?? 0, 1)
            let stepX = size.width / CGFloat(windowed.count - 1)

            func point(_ index: Int, _ value: Double) -> CGPoint {
                CGPoint(
                    x: CGFloat(index) * stepX,
                    y: size.height - CGFloat(value / maxValue) * size.height)
            }

            var areaPath = Path()
            areaPath.move(to: CGPoint(x: 0, y: size.height))
            for (index, value) in windowed.enumerated() {
                areaPath.addLine(to: point(index, value))
            }
            areaPath.addLine(to: CGPoint(x: size.width, y: size.height))
            areaPath.closeSubpath()
            context.fill(areaPath, with: .color(strokeColor.opacity(0.25)))

            let averages = Self.runningAverage(of: windowed)
            var linePath = Path()
            linePath.move(to: point(0, averages[0]))
            for index in averages.indices.dropFirst() {
                linePath.addLine(to: point(index, averages[index]))
            }
            context.stroke(linePath, with: .color(averageStrokeColor), lineWidth: 1.5)
        }
    }

    /// Caps the drawn series at one sample per point of width, taking the
    /// most recent `width` samples — draw cost then tracks the graph's own
    /// size rather than however much history the engine happens to be
    /// retaining (up to 5 minutes' worth). Left-pads with zeros when there
    /// isn't `width` samples yet, so the graph doesn't rescale/stretch while
    /// it's still filling.
    private static func windowed(_ history: [Double], to width: Int) -> [Double] {
        guard width > 0 else { return [] }
        if history.count >= width {
            return Array(history.suffix(width))
        }
        return Array(repeating: 0, count: width - history.count) + history
    }

    private static func runningAverage(of values: [Double]) -> [Double] {
        guard !values.isEmpty else { return [] }
        let twoSecondsTicks = AppTiming.ticksPerSecond * 2
        var result: [Double] = []
        var sum = 0.0
        var count = 0
        for (index, value) in values.enumerated() {
            sum += value
            count += 1
            if count >= twoSecondsTicks {
                result.append(sum / Double(count))
                count -= 1
                sum -= values[index - twoSecondsTicks + 1]
            } else {
                result.append(0)
            }
        }
        return result
    }
}
