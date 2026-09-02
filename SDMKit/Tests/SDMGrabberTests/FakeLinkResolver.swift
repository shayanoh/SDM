import Foundation

@testable import SDMCore

final class FakeLinkResolver: LinkResolver, @unchecked Sendable {
    var handledHosts: Set<String> = [
        "youtube.com", "www.youtube.com", "youtu.be", "m.youtube.com",
    ]
    var resolveResult: @Sendable (URL) throws -> ResolvedTarget
    var refreshResult: @Sendable (String) throws -> RefreshedFormat = { id in
        RefreshedFormat(url: URL(string: "https://gv/\(id)")!, filesize: nil, formatID: id)
    }

    init(_ f: @escaping @Sendable (URL) throws -> ResolvedTarget) { resolveResult = f }

    func canHandle(_ url: URL) -> Bool {
        url.host.map { handledHosts.contains($0) } ?? false
    }
    func resolve(_ url: URL) async throws -> ResolvedTarget { try resolveResult(url) }
    func refresh(sourceURL: URL, formatID: String) async throws -> RefreshedFormat {
        try refreshResult(formatID)
    }
}

func singleMedia(videoID: String, title: String, formats: [MediaFormat]) -> ResolvedTarget {
    .single(
        ResolvedMedia(
            extractor: "youtube", videoID: videoID, title: title, durationSeconds: 100,
            formats: formats))
}

func vf(
    _ id: String, _ h: Int, _ v: VideoCodec, _ c: MediaContainer, size: Int64 = 1000
) -> MediaFormat {
    MediaFormat(
        id: id, kind: .videoOnly, height: h, width: h * 16 / 9, vcodec: v, acodec: nil,
        container: c, filesize: size, filesizeApprox: nil, tbr: 1000,
        url: URL(string: "https://gv/\(id)")!)
}

func af(_ id: String, _ a: AudioCodec, _ c: MediaContainer, size: Int64 = 100) -> MediaFormat {
    MediaFormat(
        id: id, kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: a, container: c,
        filesize: size, filesizeApprox: nil, tbr: 128, url: URL(string: "https://gv/\(id)")!)
}
