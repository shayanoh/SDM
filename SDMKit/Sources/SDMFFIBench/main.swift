import Foundation
import SDMCore
import SDMEngine

// Benchmark program for the Rust core as the app uses it: Swift calling the
// generated UniFFI bindings. One scenario:
//
//   SDMFFIBench ffi-snapshot   1,000 disabled items in 20 packages, then
//                              600 x (tick + snapshot) across the boundary
//
// Nothing is downloaded: every item is disabled, so the scheduler never starts
// one and neither the network transport nor the Trash is ever used. State goes
// to a temp state file; nothing under ~/Library/Application Support/SDM is
// touched. `SDM_BENCH_SCALE` (a positive integer, default 1) divides the
// iteration count for quick debugging; the recorded run leaves it unset.

enum BenchError: Error, CustomStringConvertible {
    case failed(String)
    var description: String {
        switch self {
        case .failed(let message): return message
        }
    }
}

func scale() throws -> Int {
    guard let raw = ProcessInfo.processInfo.environment["SDM_BENCH_SCALE"] else { return 1 }
    guard let value = Int(raw), value >= 1 else {
        throw BenchError.failed("SDM_BENCH_SCALE must be a positive integer, got \(raw)")
    }
    return value
}

func runFFISnapshot(directory: URL) async throws -> String {
    let itemCount = 1000
    let packageCount = 20
    let iterations = 600 / (try scale())

    let settings = EngineSettings(
        maxConcurrent: 8, segmentsPerItem: 8, globalMaxConnections: 64,
        maxConnectionsPerHost: 64, downloadFolder: directory,
        checkpointIntervalBytes: 8 * 1024 * 1024, persistDebounceTicks: 10,
        minSegmentSizeBytes: 10 * 1024 * 1024, fileCheckIntervalTicks: 25)
    let engine = try DownloadEngine(
        config: EngineConfig(
            stateFile: directory.appendingPathComponent("state.json"),
            settings: settings, binarySearchPaths: [], cookieSource: .none,
            maxPlaylistVideos: 50, ytDlpExtraArguments: []))

    for packageIndex in 0..<packageCount {
        var items: [DownloadItem] = []
        for index in 0..<(itemCount / packageCount) {
            let name = "file\(index).bin"
            guard let url = URL(string: "https://example.com/p\(packageIndex)/\(name)") else {
                throw BenchError.failed("bad URL for \(name)")
            }
            let component = FileComponent(
                id: UUID(), url: url, partFilename: name, totalBytes: nil,
                completed: RangeSet(ranges: []), validator: nil, origin: .http,
                isResumable: nil, lastError: nil)
            items.append(
                DownloadItem(
                    id: UUID(), components: [component], outputFilename: name, sourceUrl: url,
                    assembly: .none, state: .queued, isEnabled: false, priority: nil,
                    position: Int64(index), metadata: nil))
        }
        try await engine.add(
            package: DownloadPackage(
                id: UUID(), name: "Package \(packageIndex)", items: items, priority: .normal,
                position: Int64(packageIndex), note: nil))
    }

    var itemsSeen = 0
    for _ in 0..<iterations {
        try await engine.tick()
        let snapshot = try await engine.snapshot()
        itemsSeen += snapshot.packages.reduce(0) { $0 + $1.items.count }
    }
    guard itemsSeen == iterations * itemCount else {
        throw BenchError.failed(
            "snapshots held \(itemsSeen) items, expected \(iterations * itemCount)")
    }
    try await engine.shutdown()
    return "scenario=ffi-snapshot ok=true items=\(itemCount) bytes=0"
}

// MARK: - Entry point

let usage = "usage: SDMFFIBench ffi-snapshot"
let arguments = CommandLine.arguments.dropFirst()
guard arguments.count == 1, arguments.first == "ffi-snapshot" else {
    FileHandle.standardError.write(Data((usage + "\n").utf8))
    exit(2)
}

let directory = FileManager.default.temporaryDirectory
    .appendingPathComponent("sdm-bench-\(UUID().uuidString)", isDirectory: true)
var exitCode: Int32 = 0
do {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    print(try await runFFISnapshot(directory: directory))
} catch {
    FileHandle.standardError.write(Data("scenario=ffi-snapshot ok=false error=\(error)\n".utf8))
    exitCode = 1
}
try? FileManager.default.removeItem(at: directory)
exit(exitCode)
