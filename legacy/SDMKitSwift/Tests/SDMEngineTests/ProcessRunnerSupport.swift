import Foundation

@testable import SDMResolve

/// Local copy of the `SDMResolveTests` fake — test targets don't share files.
final class FakeProcessRunner: ProcessRunner, @unchecked Sendable {
    struct Call: Sendable {
        var executable: URL
        var arguments: [String]
    }
    private let lock = NSLock()
    private var _calls: [Call] = []
    var calls: [Call] { lock.withLock { _calls } }
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

func ok(_ stdout: Data, stderr: Data = Data()) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: stdout, stderr: stderr, exitCode: 0))
}

func fail(_ stderr: String, exitCode: Int32 = 1) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: Data(), stderr: Data(stderr.utf8), exitCode: exitCode))
}
