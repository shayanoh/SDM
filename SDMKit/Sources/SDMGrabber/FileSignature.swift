import Foundation

/// A magic-byte identification of a small header sample. Spec §7.2 stage 2.
public enum FileSignature: Equatable, Sendable {
    case zip
    case rar
    case gzip
    case mp4
    case mkv
    case pdf
    case html
    case unknown

    public static func detect(in data: Data) -> FileSignature {
        let bytes = [UInt8](data.prefix(16))
        if bytes.starts(with: [0x50, 0x4B, 0x03, 0x04])
            || bytes.starts(with: [0x50, 0x4B, 0x05, 0x06])
        {
            return .zip
        }
        if bytes.starts(with: [0x52, 0x61, 0x72, 0x21]) { return .rar }
        if bytes.starts(with: [0x1F, 0x8B]) { return .gzip }
        if bytes.count >= 8, bytes[4] == 0x66, bytes[5] == 0x74, bytes[6] == 0x79, bytes[7] == 0x70
        {
            return .mp4
        }
        if bytes.starts(with: [0x1A, 0x45, 0xDF, 0xA3]) { return .mkv }
        if bytes.starts(with: [0x25, 0x50, 0x44, 0x46]) { return .pdf }
        if let text = String(data: data.prefix(512), encoding: .utf8)?.lowercased(),
            text.contains("<!doctype html") || text.contains("<html")
        {
            return .html
        }
        return .unknown
    }

    /// Whether this signature is consistent with a claimed file extension.
    /// `.unknown` never contradicts — there was not enough data to sniff, so
    /// it must not itself trigger a "faulty" verdict.
    public func matches(extension ext: String) -> Bool {
        switch (self, ext.lowercased()) {
        case (.unknown, _): return true
        case (.zip, "zip"): return true
        case (.rar, "rar"): return true
        case (.gzip, "gz"), (.gzip, "tgz"): return true
        case (.mp4, "mp4"), (.mp4, "m4v"): return true
        case (.mkv, "mkv"): return true
        case (.pdf, "pdf"): return true
        default: return false
        }
    }
}
