// Tiny dependency-free LZFSE CLI used by scripts/vendor-binaries.sh to
// compress the vendored ffmpeg / qjs binaries the same way the app inflates
// them (SDMResolve/LZFSE.swift). Build: `swiftc -O main.swift -o lzfse-pack`.
// Usage: lzfse-pack compress|decompress <in> <out>

import Compression
import Foundation

func transform(_ source: Data, operation: compression_stream_operation) -> Data? {
    guard !source.isEmpty else { return Data() }
    let streamPointer = UnsafeMutablePointer<compression_stream>.allocate(capacity: 1)
    defer { streamPointer.deallocate() }
    var stream = streamPointer.pointee
    guard compression_stream_init(&stream, operation, COMPRESSION_LZFSE) == COMPRESSION_STATUS_OK
    else { return nil }
    defer { compression_stream_destroy(&stream) }

    let bufferSize = 1 << 20
    let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
    defer { dst.deallocate() }

    var output = Data()
    return source.withUnsafeBytes { (src: UnsafeRawBufferPointer) -> Data? in
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
                return nil
            }
        }
    }
}

let args = CommandLine.arguments
guard args.count == 4, args[1] == "compress" || args[1] == "decompress" else {
    FileHandle.standardError.write(Data("usage: lzfse-pack compress|decompress <in> <out>\n".utf8))
    exit(2)
}
guard let input = FileManager.default.contents(atPath: args[2]) else {
    FileHandle.standardError.write(Data("cannot read \(args[2])\n".utf8))
    exit(1)
}
let op = args[1] == "compress" ? COMPRESSION_STREAM_ENCODE : COMPRESSION_STREAM_DECODE
guard let result = transform(input, operation: op) else {
    FileHandle.standardError.write(Data("lzfse \(args[1]) failed\n".utf8))
    exit(1)
}
try result.write(to: URL(fileURLWithPath: args[3]))
