import SDMCore
import SDMEngine
import SwiftUI

/// The live content of SDM's Touch Bar item. Hosted inside an
/// `NSHostingView` by `TouchBarController` — everything here reuses the
/// sidebar's own views/formulas (`Sparkline`, `formatted`/`formattedBytes`,
/// `SDMApp.overallFraction`'s item filter) rather than re-deriving them, so
/// the Touch Bar can never show a number the sidebar disagrees with.
///
/// `TouchBarController` only ever decides whether to install this at all
/// (see its three-state doc comment); once installed, `@Observable`
/// `EngineController` keeps this live on its own, same as any SwiftUI view.
struct TouchBarContentView: View {
    @Environment(EngineController.self) private var controller
    @Environment(ThemeStore.self) private var themeStore

    /// Touch Bar always renders on a black strip — there is no light/dark
    /// toggle to follow, so this always resolves the dark theme for contrast.
    private var theme: Theme { themeStore.resolved(for: .dark) }

    private var allItems: [ItemSnapshot] { controller.snapshot.packages.flatMap(\.items) }

    private var isAnyRunning: Bool { allItems.contains { $0.state == .running } }

    /// Same set `SDMApp.overallFraction` averages over (`SDMApp.swift:310-317`).
    private var ringItems: [ItemSnapshot] {
        allItems.filter {
            ($0.state == .running || $0.state == .queued || $0.state == .completed)
                && ($0.totalBytes ?? 0) > 0
        }
    }

    private var overallFraction: Double {
        guard !ringItems.isEmpty else { return 0 }
        return ringItems.reduce(0.0) { $0 + $1.fractionCompleted } / Double(ringItems.count)
    }

    private var downloadedOfTotalText: String {
        let downloaded = ringItems.reduce(Int64(0)) { $0 + $1.completed.totalBytes }
        let total = ringItems.reduce(Int64(0)) { $0 + ($1.totalBytes ?? 0) }
        return "\(formattedBytes(downloaded)) / \(formattedBytes(total))"
    }

    /// Mirrors `PackagesBottomBar.downloadableItems`
    /// (`PackagesListView.swift:1180-1187`) exactly — the pause/resume
    /// button here must behave identically to the one on the downloads tab.
    private var downloadableItems: [ItemSnapshot] {
        allItems.filter {
            switch $0.state {
            case .queued, .running, .stopped: return true
            case .completed, .failed: return false
            }
        }
    }

    private var allDownloadableStopped: Bool {
        downloadableItems.allSatisfy { $0.state == .stopped }
    }

    /// "Waiting to be downloaded": not completed, not disabled — independent
    /// of `downloadableItems` above, which Pause/Resume All's own semantics
    /// define and which this text does not need to match.
    private var waitingItems: [ItemSnapshot] {
        allItems.filter { $0.isEnabled && $0.state != .completed }
    }

    private var waitingPackageCount: Int {
        controller.snapshot.packages.filter { package in
            package.items.contains { $0.isEnabled && $0.state != .completed }
        }.count
    }

    var body: some View {
        HStack(spacing: 12) {
            Button {
                Task {
                    if allDownloadableStopped {
                        await controller.resumeAll()
                    } else {
                        await controller.pauseAll()
                    }
                }
            } label: {
                Image(systemName: allDownloadableStopped ? "play.fill" : "pause.fill")
            }
            .disabled(downloadableItems.isEmpty)

            if isAnyRunning {
                ProgressView(value: overallFraction)
                    .progressViewStyle(.circular)
                    .controlSize(.mini)
                    .tint(theme.accentColor)
                Text(downloadedOfTotalText)
                    .font(.callout.monospacedDigit())
                    .fixedSize()
                Sparkline(samples: controller.snapshot.globalHistory, color: theme.graphStrokeColor)
                    .frame(maxWidth: .infinity)
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.callout.monospacedDigit())
                    .fixedSize()
            } else {
                Text("\(waitingPackageCount) packages, \(waitingItems.count) items waiting")
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
        .frame(width: 480, height: 30)
    }
}
