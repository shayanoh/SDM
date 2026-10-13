import Compression
import Foundation

public enum LZFSEError: Error, Sendable { case failed }

/// Thin wrapper over `Compression`'s LZFSE stream. Used to ship the bundled
/// `ffmpeg` / `qjs` binaries compressed inside the `.app` and inflate them
/// into the managed `bin/` directory on first run.
public enum LZFSE {
    public static func compress(_ data: Data) throws -> Data {
        try transform(
            data, operation: COMPRESSION_STREAM_ENCODE, dstCapacity: max(64 << 10, data.count))
    }

    public static func decompress(_ data: Data, expandedSizeHint: Int = 64 << 20) throws -> Data {
        try transform(
            data, operation: COMPRESSION_STREAM_DECODE,
            dstCapacity: max(expandedSizeHint, data.count * 4))
    }

    private static func transform(
        _ source: Data, operation: compression_stream_operation, dstCapacity: Int
    ) throws -> Data {
        guard !source.isEmpty else { return Data() }

        let streamPointer = UnsafeMutablePointer<compression_stream>.allocate(capacity: 1)
        defer { streamPointer.deallocate() }
        var stream = streamPointer.pointee
        guard
            compression_stream_init(&stream, operation, COMPRESSION_LZFSE) == COMPRESSION_STATUS_OK
        else { throw LZFSEError.failed }
        defer { compression_stream_destroy(&stream) }

        let bufferSize = max(64 << 10, min(dstCapacity, 4 << 20))
        let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { dst.deallocate() }

        var output = Data()
        return try source.withUnsafeBytes { (src: UnsafeRawBufferPointer) throws -> Data in
            stream.src_ptr = src.bindMemory(to: UInt8.self).baseAddress!
            stream.src_size = src.count
            let flags = Int32(COMPRESSION_STREAM_FINALIZE.rawValue)
            while true {
                stream.dst_ptr = dst
                stream.dst_size = bufferSize
                let status = compression_stream_process(&stream, flags)
                switch status {
                case COMPRESSION_STATUS_OK, COMPRESSION_STATUS_END:
                    output.append(dst, count: bufferSize - stream.dst_size)
                    if status == COMPRESSION_STATUS_END { return output }
                default:
                    throw LZFSEError.failed
                }
            }
        }
    }
}
