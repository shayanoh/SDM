//
//  ContentView.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import SDMCore
import SDMEngine
import SwiftUI

struct ContentView: View {
    @Environment(EngineController.self) private var controller
    @State private var urlText = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("https://example.com/file.bin", text: $urlText)
                    .textFieldStyle(.roundedBorder)
                Button("Add") {
                    let text = urlText
                    urlText = ""
                    Task { await controller.addDownload(urlString: text) }
                }
                .disabled(urlText.isEmpty)
            }
            .padding()

            Divider()

            List {
                ForEach(controller.snapshot.packages) { package in
                    Section(package.name) {
                        ForEach(package.items) { item in
                            ItemRow(item: item, controller: controller)
                        }
                    }
                }
            }

            Divider()

            HStack {
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.title3.monospacedDigit())
                Spacer()
                Text("\(controller.snapshot.packages.count) packages")
                    .foregroundStyle(.secondary)
            }
            .padding()
        }
        .frame(minWidth: 640, minHeight: 420)
    }
}

private struct ItemRow: View {
    let item: ItemSnapshot
    let controller: EngineController

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                // Spec §6.1: enable/disable and start/stop are one axis, so
                // this button *is* the enabled toggle. Without a caller,
                // `EngineController.setEnabled` — and with it the whole
                // preempt/resume path — could not be exercised by hand at all.
                Button(item.isEnabled ? "Stop" : "Start") {
                    let enabled = !item.isEnabled
                    let id = item.id
                    Task { await controller.setEnabled(enabled, for: id) }
                }
                .controlSize(.small)
                Text(item.filename).lineLimit(1)
                resumabilityBadge
                Spacer()
                Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Text(formatted(item.bytesPerSecond))
                    .font(.caption.monospacedDigit())
            }
            SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0)
                .frame(height: 6)
            statusLine
        }
        .padding(.vertical, 2)
    }

    /// Surfaces `item.state`, which was never rendered. `.failed(reason:)` is
    /// the one state carrying a user-actionable message, so without this a
    /// stuck item looked identical to a working one.
    @ViewBuilder
    private var statusLine: some View {
        HStack(spacing: 6) {
            Text(Self.describe(item))
                .font(.caption)
                .foregroundStyle(isFailed ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
            if let checkpointFailure = item.checkpointFailure {
                // Not a failure of the download, but it means a crash would
                // lose everything transferred so far.
                Text(checkpointFailure)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .lineLimit(2)
            }
        }
    }

    private var isFailed: Bool {
        if case .failed = item.state { return true }
        return false
    }

    private static func describe(_ item: ItemSnapshot) -> String {
        switch item.state {
        // A user-disabled item and a scheduler-preempted one are both
        // `.queued` (spec §6.1 collapses them onto one axis), so the enabled
        // flag is what tells them apart in the UI.
        case .queued: return item.isEnabled ? "Queued" : "Stopped"
        case .running: return "Running"
        case .completed: return "Completed"
        case .failed(let reason): return "Failed — \(reason)"
        }
    }

    /// Renders `isResumable`'s three states honestly: `nil` (not yet probed)
    /// shows nothing rather than implying "not resumable", which is the one
    /// state that actually warrants a warning.
    @ViewBuilder
    private var resumabilityBadge: some View {
        switch item.isResumable {
        case false:
            Text("not resumable")
                .font(.caption)
                .foregroundStyle(.secondary)
        case true, nil:
            EmptyView()
        }
    }
}

/// Renders the completed `RangeSet` directly, rasterized to the bar's pixel
/// width so it stays correct at any segment count. See spec §9.4.
struct SegmentedProgressBar: View {
    let completed: RangeSet
    let total: Int64

    var body: some View {
        Canvas { context, size in
            let background = Path(
                roundedRect: CGRect(origin: .zero, size: size),
                cornerRadius: size.height / 2
            )
            context.fill(background, with: .color(.secondary.opacity(0.25)))

            guard total > 0 else { return }
            for range in completed.ranges {
                let x = size.width * CGFloat(range.start) / CGFloat(total)
                let width = size.width * CGFloat(range.length) / CGFloat(total)
                context.fill(
                    Path(CGRect(x: x, y: 0, width: max(width, 0.5), height: size.height)),
                    with: .color(.accentColor)
                )
            }
        }
    }
}

private func formatted(_ bytesPerSecond: Double) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .binary
    return formatter.string(fromByteCount: Int64(bytesPerSecond)) + "/s"
}
