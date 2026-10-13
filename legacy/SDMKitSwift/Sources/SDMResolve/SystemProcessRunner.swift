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
            // `onCancel` above already sent SIGTERM; let the child flush.
            await drainAfterTerminate(exited)
            throw CancellationError()
        }

        try? stdoutPipe.fileHandleForReading.close()
        try? stderrPipe.fileHandleForReading.close()
        return ProcessOutput(
            stdout: stdout.snapshot(), stderr: stderr.snapshot(),
            exitCode: process.terminationStatus)
    }

    public func runStreaming(
        executable: URL, arguments: [String], timeout: Duration,
        onLine: @Sendable @escaping (String) -> Void
    ) async throws -> Int32 {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.environment = Self.childEnvironment(for: executable)
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        let streamer = LineStreamer(onLine: onLine)
        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { streamer.feed(chunk) }
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { streamer.feed(chunk) }
        }

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
            // `onCancel` above already sent SIGTERM; let the child flush.
            await drainAfterTerminate(exited)
            throw CancellationError()
        }

        try? stdoutPipe.fileHandleForReading.close()
        try? stderrPipe.fileHandleForReading.close()
        streamer.flush()
        return process.terminationStatus
    }
}

/// Splits a byte stream into `\n`-delimited lines, emitting each completed
/// line via `onLine`. Thread-safe for the pipe readability handlers.
private final class LineStreamer: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private let onLine: @Sendable (String) -> Void

    init(onLine: @escaping @Sendable (String) -> Void) { self.onLine = onLine }

    func feed(_ chunk: Data) {
        let lines: [String] = lock.withLock {
            buffer.append(chunk)
            var completed: [String] = []
            while let newline = buffer.firstIndex(of: 0x0A) {
                let lineData = buffer[buffer.startIndex..<newline]
                completed.append(
                    String(decoding: lineData, as: UTF8.self)
                        .trimmingCharacters(in: CharacterSet(charactersIn: "\r")))
                buffer.removeSubrange(buffer.startIndex...newline)
            }
            return completed
        }
        for line in lines { onLine(line) }
    }

    func flush() {
        let trailing: String? = lock.withLock {
            guard !buffer.isEmpty else { return nil }
            let s = String(decoding: buffer, as: UTF8.self)
            buffer.removeAll()
            return s
        }
        if let trailing, !trailing.isEmpty { onLine(trailing) }
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

    /// Like `wait()` but ignores task cancellation — used to drain a child
    /// that has just been sent SIGTERM so it can flush before we return.
    func waitIgnoringCancellation() async {
        let id = UUID()
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
    }
}

/// After SIGTERM, give the child a bounded window to exit on its own so it
/// can flush any resume state (yt-dlp's `.ytdl` / `.part`) before a
/// rescheduled run touches the same files. Runs detached so the caller's
/// own cancellation does not cut the wait short.
private func drainAfterTerminate(_ exited: ExitSignal) async {
    let drain = Task.detached {
        await withTaskGroup(of: Void.self) { group in
            group.addTask { await exited.waitIgnoringCancellation() }
            group.addTask { try? await Task.sleep(for: .seconds(3)) }
            _ = await group.next()
            group.cancelAll()
        }
    }
    await drain.value
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
