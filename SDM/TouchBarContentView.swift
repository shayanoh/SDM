import SDMCore
import SDMEngine
import SwiftUI

/// The live, read-only content of SDM's Touch Bar — ring, byte count,
/// sparkline, speed, or the idle waiting text. Hosted inside an
/// `NSHostingView` by `TouchBarController`, which owns a *separate*,
/// native `NSButtonTouchBarItem` for pause/resume: a SwiftUI `Button`
/// hosted the same way does not reliably receive a completed tap in a
/// Touch Bar item (confirmed — it highlights on touch-down but its action
/// never fires), so the one interactive control here is plain AppKit.
///
/// Everything below reuses the sidebar's own views and figures (`Sparkline`,
/// `formatted`/`formattedBytes`, `EngineStats`) rather than re-deriving
/// them, so the Touch Bar can never show a number the sidebar disagrees
/// with.
///
/// `TouchBarController` only ever decides whether to install the bar at all
/// (see its three-state doc comment); once installed, `@Observable`
/// `EngineController` keeps this live on its own, same as any SwiftUI view.
struct TouchBarContentView: View {
    @Environment(EngineController.self) private var controller
    @Environment(ThemeStore.self) private var themeStore

    /// Touch Bar always renders on a black strip — there is no light/dark
    /// toggle to follow, so this always resolves the dark theme for contrast.
    private var theme: Theme { themeStore.resolved(for: .dark) }

    private var stats: EngineStats { controller.stats }

    private var downloadedOfTotalText: String {
        "\(formattedBytes(stats.ringCompletedBytes)) / \(formattedBytes(stats.ringTotalBytes))"
    }

    var body: some View {
        HStack(spacing: 12) {
            if stats.runningCount > 0 {
                ProgressView(value: stats.overallFraction)
                    .progressViewStyle(.circular)
                    .controlSize(.mini)
                    .tint(theme.accentColor)
                Text(downloadedOfTotalText)
                    .font(.callout.monospacedDigit())
                    .fixedSize()
                Sparkline(samples: stats.globalHistory, color: theme.graphStrokeColor)
                    .frame(maxWidth: .infinity)
                Text(formatted(stats.globalBytesPerSecond))
                    .font(.callout.monospacedDigit())
                    .fixedSize()
            } else {
                Text(
                    "\(stats.waitingPackageCount) packages, \(stats.waitingItemCount) items waiting"
                )
                .foregroundStyle(theme.textSecondaryColor)
                .fixedSize()
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, 8)
        // The Touch Bar has no scrolling and SwiftUI's `NSHostingView` sizes
        // itself to its content's intrinsic size, not to whatever frame
        // `NSHostingView.frame` is assigned from the AppKit side — without
        // an explicit width here every element (including the sparkline's
        // `maxWidth: .infinity`) just hugs its minimum size, cramming
        // everything to the left. This fixed width stands in for "fill the
        // available Touch Bar width."
        .frame(width: 440, height: 30)
    }
}
