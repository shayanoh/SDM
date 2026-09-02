import Foundation
import Testing

@testable import SDMResolve

@Test func capturesStdoutAndZeroExit() async throws {
    let runner = SystemProcessRunner()
    let out = try await runner.run(
        executable: URL(fileURLWithPath: "/bin/echo"),
        arguments: ["hello"], timeout: .seconds(5))
    #expect(out.exitCode == 0)
    #expect(String(decoding: out.stdout, as: UTF8.self) == "hello\n")
}

@Test func capturesNonZeroExit() async throws {
    let runner = SystemProcessRunner()
    let out = try await runner.run(
        executable: URL(fileURLWithPath: "/bin/sh"),
        arguments: ["-c", "exit 3"], timeout: .seconds(5))
    #expect(out.exitCode == 3)
}

@Test func timesOutALongProcess() async {
    let runner = SystemProcessRunner()
    await #expect(throws: ProcessRunError.timedOut) {
        try await runner.run(
            executable: URL(fileURLWithPath: "/bin/sleep"),
            arguments: ["10"], timeout: .milliseconds(200))
    }
}

@Test func throwsLaunchFailedForMissingExecutable() async {
    let runner = SystemProcessRunner()
    await #expect(throws: (any Error).self) {
        try await runner.run(
            executable: URL(fileURLWithPath: "/nonexistent/xyz"),
            arguments: [], timeout: .seconds(1))
    }
}

@Test func cancellationTerminatesTheProcess() async throws {
    let runner = SystemProcessRunner()
    let task = Task {
        try await runner.run(
            executable: URL(fileURLWithPath: "/bin/sleep"),
            arguments: ["10"], timeout: .seconds(30))
    }
    try await Task.sleep(for: .milliseconds(100))
    task.cancel()
    await #expect(throws: (any Error).self) { try await task.value }
}

@Test func childEnvironmentPrependsToolDirsAndGuaranteesHome() {
    let env = SystemProcessRunner.childEnvironment(
        for: URL(fileURLWithPath: "/opt/custom/bin/yt-dlp"))
    let path = env["PATH"] ?? ""
    #expect(path.hasPrefix("/opt/custom/bin:"))
    #expect(path.contains("/opt/homebrew/bin"))
    #expect(path.contains("/usr/local/bin"))
    #expect(path.contains("/usr/bin"))
    #expect(!(env["HOME"] ?? "").isEmpty)
}
