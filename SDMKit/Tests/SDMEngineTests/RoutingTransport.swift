import Foundation

@testable import SDMEngine

/// Dispatches a fetch to one of two `FakeOrigin`s by request host, so a
/// multi-component item whose parts live on different hosts can be driven in
/// one test.
struct TwoHostRouter: HTTPTransport {
    let video: FakeOrigin
    let audio: FakeOrigin
    let videoHost: String
    let audioHost: String

    init(
        videoHost: String, videoPayload: Data, videoBehavior: FakeOrigin.Behavior = .init(),
        audioHost: String, audioPayload: Data, audioBehavior: FakeOrigin.Behavior = .init()
    ) {
        self.videoHost = videoHost
        self.audioHost = audioHost
        self.video = FakeOrigin(payload: videoPayload, behavior: videoBehavior)
        self.audio = FakeOrigin(payload: audioPayload, behavior: audioBehavior)
    }

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        switch request.url.host {
        case videoHost: return try await video.fetch(request)
        case audioHost: return try await audio.fetch(request)
        default: return try await video.fetch(request)
        }
    }
}
