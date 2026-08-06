import SwiftUI

struct SettingsView: View {
    @Environment(EngineController.self) private var controller
    @State private var maxConcurrent = EngineSettingsStore.maxConcurrent
    @State private var segmentsPerItem = EngineSettingsStore.segmentsPerItem
    @State private var globalMaxConnections = EngineSettingsStore.globalMaxConnections
    @State private var maxConnectionsPerHost = EngineSettingsStore.maxConnectionsPerHost
    @State private var clipboardWatchingEnabled = GrabberSettings.clipboardWatchingEnabled
    @State private var autoAddAndStartOnGrab = GrabberSettings.autoAddAndStartOnGrab
    @State private var deepSniffEnabled = GrabberSettings.deepSniffEnabled

    var body: some View {
        Form {
            Section("Downloads") {
                Stepper(
                    "Max concurrent downloads: \(maxConcurrent)", value: $maxConcurrent, in: 1...20)
                Stepper(
                    "Segments per file: \(segmentsPerItem)", value: $segmentsPerItem, in: 1...64)
                Stepper(
                    "Global max connections: \(globalMaxConnections)",
                    value: $globalMaxConnections, in: 1...256
                )
                Stepper(
                    "Max connections per host: \(maxConnectionsPerHost)",
                    value: $maxConnectionsPerHost, in: 1...64
                )
            }
            Section("Linkgrabber") {
                Toggle("Watch clipboard for links", isOn: $clipboardWatchingEnabled)
                Toggle("Auto-add and start on grab", isOn: $autoAddAndStartOnGrab)
                Toggle("Deep sniff (stage 2)", isOn: $deepSniffEnabled)
            }
        }
        .padding()
        .frame(width: 420)
        .onChange(of: maxConcurrent) { _, _ in applyEngineSettings() }
        .onChange(of: segmentsPerItem) { _, _ in applyEngineSettings() }
        .onChange(of: globalMaxConnections) { _, _ in applyEngineSettings() }
        .onChange(of: maxConnectionsPerHost) { _, _ in applyEngineSettings() }
        .onChange(of: clipboardWatchingEnabled) { _, new in
            GrabberSettings.clipboardWatchingEnabled = new
        }
        .onChange(of: autoAddAndStartOnGrab) { _, new in GrabberSettings.autoAddAndStartOnGrab = new
        }
        .onChange(of: deepSniffEnabled) { _, new in GrabberSettings.deepSniffEnabled = new }
    }

    private func applyEngineSettings() {
        EngineSettingsStore.maxConcurrent = maxConcurrent
        EngineSettingsStore.segmentsPerItem = segmentsPerItem
        EngineSettingsStore.globalMaxConnections = globalMaxConnections
        EngineSettingsStore.maxConnectionsPerHost = maxConnectionsPerHost
        Task { await controller.applyStoredSettings() }
    }
}
