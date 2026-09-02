import Foundation

/// `Foundation.Process` implementation. Unsandboxed target — this is
/// allowed. Kills the child on timeout or task cancellation.
public struct SystemProcessRunner: ProcessRunner {
    public init() {}

    /// A `.app` launched from Finder inherits a near-empty `PATH` and may be
    /// missing `HOME`. yt-dlp needs `HOME` for its cache/config and shells
    /// out to `ffmpeg` and a JS runtime (`deno`/`node`) for YouTube's nsig
    /// challenge — without those on `PATH` its extraction degrades and
    /// YouTube starts returning anti-bot walls. So we hand the child a real
    /// environment: whatever we inherited, plus the standard tool
    /// directories (and the executable's own directory) prepended to `PATH`,
    /// and `HOME` guaranteed.
    static func childEnvironment(for executable: URL) -> [String: String] {
        var env = ProcessInfo.processInfo.environment

        let toolDirs = [
            executable.deletingLastPathComponent().path,
            "/opt/homebrew/bin", "/opt/homebrew/sbin",
            "/usr/local/bin", "/usr/local/sbin",
            "/opt/local/bin", "/opt/local/sbin",
            "/usr/bin", "/bin", "/usr/sbin", "/sbin",
            (NSHomeDirectory() as NSString).appendingPathComponent(".local/bin"),
        ]
        let existing = (env["PATH"] ?? "").split(separator: ":").map(String.init)
        var seen = Set<String>()
        let merged = (toolDirs + existing).filter { seen.insert($0).inserted && !$0.isEmpty }
        env["PATH"] = merged.joined(separator: ":")

        if (env["HOME"] ?? "").isEmpty { env["HOME"] = NSHomeDirectory() }
        return env
    }

    public func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.environment = Self.childEnvironment(for: executable)
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        // Read the pipes on background queues so a large output can never
        // deadlock against a full pipe buffer while the process runs.
        let stdout = DataAccumulator()
        let stderr = DataAccumulator()
        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { stdout.append(chunk) }
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { stderr.append(chunk) }
        }

        // Bridge process termination to an async signal. Set before `run()`
        // so a fast-exiting process cannot fire before we are listening.
        let exited = ExitSignal()
        process.terminationHandler = { _ in exited.fire() }

        do {
            try process.run()
        } catch {
            stdoutPipe.fileHandleForReading.readabilityHandler = nil
            stderrPipe.fileHandleForReading.readabilityHandler = nil
            throw ProcessRunError.launchFailed(error.localizedDescription)
        }

        let timedOut = await withTaskCancellationHandler {
            await withThrowingTaskGroup(of: Bool.self) { group -> Bool in
                group.addTask {
                    await exited.wait()
                    return false
                }
                group.addTask {
                    try? await Task.sleep(for: timeout)
                    return true
                }
                let first = (try? await group.next()) ?? true
                group.cancelAll()
                return first
            }
        } onCancel: {
            process.terminate()
        }

        if timedOut {
            process.terminate()
            await exited.wait()
            throw ProcessRunError.timedOut
        }
        if Task.isCancelled {
            throw CancellationError()
        }

        try? stdoutPipe.fileHandleForReading.close()
        try? stderrPipe.fileHandleForReading.close()
        return ProcessOutput(
            stdout: stdout.snapshot(), stderr: stderr.snapshot(),
            exitCode: process.terminationStatus)
    }
}

/// One-shot termination signal, safe to fire before or after a waiter
/// arrives. `wait()` also returns promptly on task cancellation so it never
/// pins a task group open waiting on a process that outlives the timeout.
private final class ExitSignal: @unchecked Sendable {
    private let lock = NSLock()
    private var hasFired = false
    private var waiters: [UUID: CheckedContinuation<Void, Never>] = [:]

    func fire() {
        lock.lock()
        hasFired = true
        let pending = waiters
        waiters.removeAll()
        lock.unlock()
        for waiter in pending.values { waiter.resume() }
    }

    func wait() async {
        let id = UUID()
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                lock.lock()
                if hasFired {
                    lock.unlock()
                    continuation.resume()
                } else {
                    waiters[id] = continuation
                    lock.unlock()
                }
            }
        } onCancel: {
            lock.lock()
            let waiter = waiters.removeValue(forKey: id)
            lock.unlock()
            waiter?.resume()
        }
    }
}

/// Thread-safe byte accumulator for the pipe readability handlers.
private final class DataAccumulator: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()

    func append(_ chunk: Data) {
        lock.lock()
        data.append(chunk)
        lock.unlock()
    }

    func snapshot() -> Data {
        lock.lock()
        defer { lock.unlock() }
        return data
    }
}
