import Foundation

public enum BinaryFetchError: Error, Equatable, Sendable {
    case http(Int)
    case transport(String)
}

/// The network seam for `ManagedBinaries`. Tests inject a fake so the update
/// path never touches the network.
public protocol BinaryFetching: Sendable {
    /// GETs `url`, following redirects, and returns the body. Throws
    /// `BinaryFetchError.http` on a non-2xx status and
    /// `BinaryFetchError.transport` on a connection failure.
    func data(from url: URL) async throws -> Data
}

public struct URLSessionBinaryFetcher: BinaryFetching {
    private let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func data(from url: URL) async throws -> Data {
        var request = URLRequest(url: url)
        request.setValue("SDM", forHTTPHeaderField: "User-Agent")
        request.timeoutInterval = 60
        do {
            let (data, response) = try await session.data(for: request)
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                throw BinaryFetchError.http(http.statusCode)
            }
            return data
        } catch let error as BinaryFetchError {
            throw error
        } catch {
            throw BinaryFetchError.transport(error.localizedDescription)
        }
    }
}
