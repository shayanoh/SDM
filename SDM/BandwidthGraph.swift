import Charts
import SwiftUI

/// Spec §9.6: Swift Charts is reserved for the one place it earns its
/// cost — everywhere else (per-row sparklines) uses a plain `Canvas`, since a
/// full `Chart` per row at hundreds of rows turns scrolling into a
/// slideshow. The filled area uses spec §10.1's `graphStroke` role, and the
/// running-average line its distinct `graphAverageStroke` role.
struct BandwidthGraph: View {
    let history: [Double]
    let strokeColor: Color
    let averageStrokeColor: Color

    var body: some View {
        Chart {
            ForEach(Array(history.enumerated()), id: \.offset) { index, value in
                AreaMark(x: .value("Tick", index), y: .value("Bytes/s", value))
                    .foregroundStyle(strokeColor.opacity(0.25))
                LineMark(x: .value("Tick", index), y: .value("Average", runningAverage[index]))
                    .foregroundStyle(averageStrokeColor)
            }
        }
        .chartXAxis(.hidden)
        .chartYAxis(.hidden)
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
