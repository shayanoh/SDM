import Foundation
import SDMCore

/// Runs spec §7.2's stage 1 (cheap probe) and, when the link looks alive and
/// deep sniff is enabled, stage 2 (magic-byte sniff, Task 5).
public struct LinkProber: Sendable {
    private let transport: any ProbeTransport
    public let deepSniffEnabled: Bool

    public init(transport: any ProbeTransport, deepSniffEnabled: Bool = true) {
        self.transport = transport
        self.deepSniffEnabled = deepSniffEnabled
    }

    public func probe(_ url: URL) async -> ProbedLink {
        var link = ProbedLink(originalURL: url, stage: .probing)

        do {
            let response = try await stageOneResponse(for: url)
            apply(response, to: &link)
        } catch {
            link.transportFailed = true
            link.stage = .done
            return link
        }

        await sniffIfNeeded(&link)
        link.stage = .done
        return link
    }

    /// HEAD first; falls back to a `Range: bytes=0-0` GET when the origin
    /// rejects or lies about HEAD.
    private func stageOneResponse(for url: URL) async throws -> ProbeResponse {
        do {
            return try await transport.send(ProbeRequest(url: url, method: .head))
        } catch {
            return try await transport.send(
                ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 1))
            )
        }
    }

    private func apply(_ response: ProbeResponse, to link: inout ProbedLink) {
        link.finalURL = response.finalURL
        link.statusCode = response.statusCode
        link.acceptsRanges = response.statusCode == 206
        if let text = response.headers["content-length"], let length = Int64(text) {
            link.contentLength = length
        }
        if let contentRange = response.headers["content-range"],
            let slash = contentRange.lastIndex(of: "/")
        {
            let total = contentRange[contentRange.index(after: slash)...]
            if total != "*", let value = Int64(total) { link.contentLength = value }
        }
        link.contentType = response.headers["content-type"]?
            .split(separator: ";").first.map(String.init)
        link.validator = response.headers["etag"] ?? response.headers["last-modified"]
        link.suggestedFilename = Self.filename(
            fromContentDisposition: response.headers["content-disposition"]
        )
    }

    private static func filename(fromContentDisposition header: String?) -> String? {
        guard let header else { return nil }
        for part in header.split(separator: ";") {
            let trimmed = part.trimmingCharacters(in: .whitespaces)
            guard trimmed.lowercased().hasPrefix("filename=") else { continue }
            let value = trimmed.dropFirst("filename=".count)
                .trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            return value.isEmpty ? nil : value
        }
        return nil
    }
}

extension LinkProber {
    fileprivate func sniffIfNeeded(_ link: inout ProbedLink) async {
        guard deepSniffEnabled, let status = link.statusCode, (200...299).contains(status) else {
            return
        }
        link.stage = .sniffing
        guard
            let response = try? await transport.send(
                ProbeRequest(
                    url: link.finalURL, method: .get, range: ByteRange(start: 0, end: 65536))
            )
        else { return }
        link.sniffedSignature = FileSignature.detect(in: response.body)
    }
}
