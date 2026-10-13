import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

func fixtureData(_ name: String) throws -> Data {
    let url = Bundle.module.url(
        forResource: name, withExtension: "json", subdirectory: "Fixtures")
    let resolved = try #require(url, "missing fixture \(name).json")
    return try Data(contentsOf: resolved)
}

func fixtureDump(_ name: String) throws -> YtDlpDump {
    try JSONDecoder().decode(YtDlpDump.self, from: fixtureData(name))
}

/// Records the arguments it was called with and replays a scripted output.
final class FakeProcessRunner: ProcessRunner, @unchecked Sendable {
    struct Call: Sendable {
        var executable: URL
        var arguments: [String]
    }
    private let lock = NSLock()
    private var _calls: [Call] = []
    var calls: [Call] {
        lock.withLock { _calls }
    }
    /// Matched against the joined argument string; first match wins.
    var responses: [(match: String, output: Result<ProcessOutput, any Error>)] = []
    var defaultOutput: Result<ProcessOutput, any Error> =
        .success(ProcessOutput(stdout: Data(), stderr: Data(), exitCode: 0))

    func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        lock.withLock { _calls.append(Call(executable: executable, arguments: arguments)) }
        let joined = arguments.joined(separator: " ")
        let picked = responses.first { joined.contains($0.match) }?.output ?? defaultOutput
        return try picked.get()
    }
}

/// Replays canned responses keyed by absolute URL string; counts requests.
final class FakeBinaryFetcher: BinaryFetching, @unchecked Sendable {
    private let lock = NSLock()
    private var _responses: [String: Result<Data, any Error>] = [:]
    private var _requestCount = 0
    private var _requestedURLs: [String] = []

    var responses: [String: Result<Data, any Error>] {
        get { lock.withLock { _responses } }
        set { lock.withLock { _responses = newValue } }
    }
    var requestCount: Int { lock.withLock { _requestCount } }
    var requestedURLs: [String] { lock.withLock { _requestedURLs } }

    func setResponse(_ result: Result<Data, any Error>, for url: URL) {
        lock.withLock { _responses[url.absoluteString] = result }
    }

    func data(from url: URL) async throws -> Data {
        let result: Result<Data, any Error>? = lock.withLock {
            _requestCount += 1
            _requestedURLs.append(url.absoluteString)
            return _responses[url.absoluteString]
        }
        guard let result else { throw BinaryFetchError.http(404) }
        return try result.get()
    }
}

extension URL {
    /// A fresh empty temp directory URL (not created on disk).
    static func tmp() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent(
            UUID().uuidString, isDirectory: true)
    }
}

func ok(_ stdout: Data, stderr: Data = Data()) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: stdout, stderr: stderr, exitCode: 0))
}

func fail(_ stderr: String, exitCode: Int32 = 1) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: Data(), stderr: Data(stderr.utf8), exitCode: exitCode))
}
