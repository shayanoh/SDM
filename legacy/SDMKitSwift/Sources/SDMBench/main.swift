import Foundation
import SDMCore
import SDMEngine

// Benchmark program for the all-Swift core. It is the twin of
// sdm-core/crates/sdm-bench/src/main.rs: the same two scenarios with the same
// parameters, each against the core's own public `FakeOrigin`, an in-memory
// state store and identical `EngineSettings`. Run it from scripts/bench.sh.
//
//   SDMBench transfer   8 items x 256 MB in one package, run to completion
//   SDMBench ticks      1,000 items in 20 packages, 8 running, 600 x (tick + snapshot)
//
// `SDM_BENCH_SCALE` (a positive integer, default 1) divides the transfer item
// size and the tick count for quick debugging; both programs read it
// identically and the recorded run leaves it unset.

enum BenchError: Error, CustomStringConvertible {
    case failed(String)
    var description: String {
        switch self {
        case .failed(let message): return message
        }
    }
}

/// Same bytes as the Rust side's `test_payload`: index-derived, so the origin
/// costs the same to build and any byte can be checked independently.
func makePayload(_ count: Int) -> Data {
    var bytes = [UInt8](repeating: 0, count: count)
    for i in 0..<count {
        bytes[i] = UInt8((i &* 31 &+ 7) % 251)
    }
    return Data(bytes)
}

/// Bytes per chunk the fake origin emits. 64 KiB on both sides (the fake's
/// default of 64 bytes would measure the stream plumbing, not the transfer).
let chunkSize = 64 * 1024

/// A gate that is never opened: waiters are kept, not resumed.
actor ClosedGate {
    private var waiters: [CheckedContinuation<Void, Never>] = []
    func wait() async {
        await withCheckedContinuation { waiters.append($0) }
    }
}

let closedGate = ClosedGate()

/// Fronts a `FakeOrigin`, and parks every body fetch forever when `gated`, so
/// running items stay running. The one-byte probe always passes through.
struct BenchTransport: HTTPTransport {
    let origin: FakeOrigin
    let gated: Bool

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        if gated {
            let isProbe = request.range.map { $0.start == 0 && $0.end == 1 } ?? false
            if !isProbe {
                await closedGate.wait()
            }
        }
        return try await origin.fetch(request)
    }
}

func makeOrigin(payload: Data) -> FakeOrigin {
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = chunkSize
    return FakeOrigin(payload: payload, behavior: behavior)
}

func makeEngine(payload: Data, folder: URL, gated: Bool) -> DownloadEngine {
    DownloadEngine(
        transport: BenchTransport(origin: makeOrigin(payload: payload), gated: gated),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 8,
            segmentsPerItem: 8,
            globalMaxConnections: 64,
            maxConnectionsPerHost: 64,
            downloadFolder: folder
        )
    )
}

func scale() throws -> Int {
    guard let raw = ProcessInfo.processInfo.environment["SDM_BENCH_SCALE"] else { return 1 }
    guard let value = Int(raw), value >= 1 else {
        throw BenchError.failed("SDM_BENCH_SCALE must be a positive integer, got \(raw)")
    }
    return value
}

func runTransfer(directory: URL) async throws -> String {
    let itemBytes = 256 * 1024 * 1024 / (try scale())
    let itemCount = 8
    let payload = makePayload(itemBytes)
    let engine = makeEngine(payload: payload, folder: directory, gated: false)

    let items = (0..<itemCount).map { index in
        DownloadItem(
            url: URL(string: "https://example.com/file\(index).bin")!,
            filename: "file\(index).bin",
            position: index)
    }
    await engine.add(DownloadPackage(name: "Batch", items: items))

    while true {
        await engine.tick()
        let snapshot = await engine.snapshot()
        let states = snapshot.packages.flatMap(\.items).map(\.state)
        for case .failed(let reason) in states {
            throw BenchError.failed("an item failed: \(reason)")
        }
        if states.count == itemCount, states.allSatisfy({ $0 == .completed }) { break }
        await Task.yield()
    }

    var total = 0
    for item in items {
        let path = directory.appendingPathComponent("Batch").appendingPathComponent(
            item.outputFilename
        )
        .path
        let attributes = try FileManager.default.attributesOfItem(atPath: path)
        let size = (attributes[.size] as? NSNumber)?.intValue ?? -1
        guard size == itemBytes else {
            throw BenchError.failed(
                "\(item.outputFilename) is \(size) bytes, expected \(itemBytes)")
        }
        total += size
    }
    return "scenario=transfer ok=true items=\(itemCount) bytes=\(total)"
}

func runTicks(directory: URL) async throws -> String {
    let itemCount = 1000
    let packageCount = 20
    let itemBytes = 64 * 1024
    let iterations = 600 / (try scale())
    let engine = makeEngine(payload: makePayload(itemBytes), folder: directory, gated: true)

    for packageIndex in 0..<packageCount {
        let perPackage = itemCount / packageCount
        let items = (0..<perPackage).map { index in
            DownloadItem(
                url: URL(string: "https://example.com/p\(packageIndex)/file\(index).bin")!,
                filename: "file\(index).bin",
                position: index)
        }
        await engine.add(
            DownloadPackage(name: "Package \(packageIndex)", items: items, position: packageIndex))
    }

    var packagesSeen = 0
    for _ in 0..<iterations {
        await engine.tick()
        let snapshot = await engine.snapshot()
        packagesSeen += snapshot.packages.count
    }
    guard packagesSeen == iterations * packageCount else {
        throw BenchError.failed(
            "snapshots held \(packagesSeen) packages, expected \(iterations * packageCount)")
    }
    return "scenario=ticks ok=true items=\(itemCount) bytes=0"
}

// MARK: - Entry point

let usage = "usage: SDMBench transfer|ticks"
let arguments = CommandLine.arguments.dropFirst()
guard arguments.count == 1, let scenario = arguments.first,
    scenario == "transfer" || scenario == "ticks"
else {
    FileHandle.standardError.write(Data((usage + "\n").utf8))
    exit(2)
}

let directory = FileManager.default.temporaryDirectory
    .appendingPathComponent("sdm-bench-\(UUID().uuidString)", isDirectory: true)
var exitCode: Int32 = 0
do {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let line =
        scenario == "transfer"
        ? try await runTransfer(directory: directory) : try await runTicks(directory: directory)
    print(line)
} catch {
    FileHandle.standardError.write(Data("scenario=\(scenario) ok=false error=\(error)\n".utf8))
    exitCode = 1
}
try? FileManager.default.removeItem(at: directory)
exit(exitCode)
