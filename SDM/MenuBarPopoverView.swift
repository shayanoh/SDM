import AppKit
import SDMCore
import SDMEngine
import SDMGrabber
import SwiftUI

/// Spec §9.7: aggregate speed, mini bandwidth graph, active downloads with
/// progress and speed, a pending-links row above the actions (the one item
/// there requiring a decision), and Pause all / Open SDM / Quit.
struct MenuBarPopoverView: View {
    @Environment(EngineController.self) private var controller
    @Environment(GrabberController.self) private var grabberController
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow
    @Binding var selection: MainWindowView.SidebarItem?

    private var theme: Theme { themeStore.resolved(for: colorScheme) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.headline.monospacedDigit())
                Spacer()
                Text("\(activeItems.count) active").font(.caption).foregroundStyle(
                    theme.textSecondaryColor)
            }
            BandwidthGraph(
                history: controller.snapshot.globalHistory, strokeColor: theme.graphStrokeColor,
                averageStrokeColor: theme.graphAverageStrokeColor
            )
            .frame(height: 32)

            Divider()

            ForEach(activeItems.prefix(5)) { item in
                HStack {
                    ProgressView(value: item.fractionCompleted)
                        .progressViewStyle(.circular)
                        .controlSize(.small)
                    Text(item.filename).lineLimit(1).font(.caption)
                    Spacer()
                    Text(formatted(item.bytesPerSecond)).font(.caption.monospacedDigit())
                }
            }

            if grabberController.snapshot.totalCount > 0 {
                Divider()
                HStack {
                    Text("\(grabberController.snapshot.totalCount) links waiting").font(.caption)
                    Spacer()
                    Button("Review") {
                        selection = .linkgrabber
                        openWindow(id: "main")
                        NSApp.activate(ignoringOtherApps: true)
                    }
                    .controlSize(.small)
                }
            }

            Divider()

            HStack {
                Button("Pause all") {
                    Task { await controller.pauseAll() }
                }
                Spacer()
                Button("Open SDM") {
                    openWindow(id: "main")
                    NSApp.activate(ignoringOtherApps: true)
                }
                Button("Quit") { NSApp.terminate(nil) }
            }
        }
        .padding()
        .frame(width: 280)
    }

    private var activeItems: [ItemSnapshot] {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }
    }
}
