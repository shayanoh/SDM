import AppKit
import Foundation

// MARK: - Messages from Chrome

struct DownloadMessage: Decodable {
    let type: String
    let url: String
}

struct ExtensionStatusMessage: Decodable {
    let type: String
    let version: String
}

// MARK: - Errors

enum NativeHostError: Error {
    case unexpectedEOF
    case invalidMessageLength
    case messageTooLarge
    case invalidURL
}

// Chrome native messaging messages are prefixed with a
// 32-bit message length.
//
// Keep this reasonably conservative. We don't need huge
// messages for a download URL.
private let maximumMessageSize = 64 * 1024 // 64 KB

// MARK: - Native Messaging I/O

private func readExactly(_ count: Int) throws -> Data {
    var data = Data()
    data.reserveCapacity(count)

    while data.count < count {
        guard let chunk = try FileHandle.standardInput.read(
            upToCount: count - data.count
        ), !chunk.isEmpty else {
            throw NativeHostError.unexpectedEOF
        }

        data.append(chunk)
    }

    return data
}

private func readMessage() throws -> Data {
    let lengthData = try readExactly(4)

    let length = lengthData.withUnsafeBytes {
        UInt32(littleEndian: $0.load(as: UInt32.self))
    }

    guard length > 0 else {
        throw NativeHostError.invalidMessageLength
    }

    guard length <= maximumMessageSize else {
        throw NativeHostError.messageTooLarge
    }

    return try readExactly(Int(length))
}

private func writeMessage<T: Encodable>(_ value: T) throws {
    let payload = try JSONEncoder().encode(value)

    guard payload.count <= maximumMessageSize else {
        throw NativeHostError.messageTooLarge
    }

    let length = UInt32(payload.count)

    var message = Data()

    withUnsafeBytes(of: length.littleEndian) {
        message.append(contentsOf: $0)
    }

    message.append(payload)

    try FileHandle.standardOutput.write(contentsOf: message)
}

// MARK: - SDM URL

private func makeSDMURL(for downloadURL: String) -> URL? {
    var components = URLComponents()

    components.scheme = "com-shayanoh-sdm"
    components.host = "download"

    components.queryItems = [
        URLQueryItem(
            name: "url",
            value: downloadURL
        )
    ]

    return components.url
}

// MARK: - Helpers

private func saveExtensionStatus(version: String) throws {
    let fileManager = FileManager.default

    guard let applicationSupport = fileManager.urls(
        for: .applicationSupportDirectory,
        in: .userDomainMask
    ).first else {
        throw NativeHostError.unexpectedEOF
    }

    let directory = applicationSupport
        .appendingPathComponent("SDM", isDirectory: true)

    try fileManager.createDirectory(
        at: directory,
        withIntermediateDirectories: true
    )

    let statusURL = directory
        .appendingPathComponent("ChromeExtensionStatus.json")

    struct Status: Encodable {
        let version: String
        let lastSeen: Date
    }

    let status = Status(
        version: version,
        lastSeen: Date()
    )

    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

    try encoder.encode(status).write(
        to: statusURL,
        options: [.atomic]
    )
}

// MARK: - Main

do {
    let data = try readMessage()

    let decoder = JSONDecoder()

    if let message = try? decoder.decode(
        ExtensionStatusMessage.self,
        from: data
    ),
    message.type == "extensionStatus" {
        try saveExtensionStatus(version: message.version)

        try writeMessage([
            "success": true
        ])

        exit(0)
    }
    else if let message = try? decoder.decode(
        DownloadMessage.self,
        from: data
    ), message.type == "download" {
        guard let downloadURL = URL(string: message.url) else {
            throw NativeHostError.invalidURL
        }

        guard let sdmURL = makeSDMURL(for: downloadURL.absoluteString) else {
            throw NativeHostError.invalidURL
        }
        
        let opened = NSWorkspace.shared.open(sdmURL)

        try writeMessage([
            "success": opened
        ])
    } else {
        throw NativeHostError.invalidURL
    }
} catch {
    fputs(
        "SDMNativeHost error: \(error)\n",
        stderr
    )

    exit(1)
}
