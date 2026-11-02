import Foundation
import Security
import os

enum SPKI {
    /// The DER prefix of a P-256 SubjectPublicKeyInfo, up to the BIT STRING
    /// that holds the 65-byte uncompressed point.
    static let p256Header = Data(hex: "3059301306072a8648ce3d020106082a8648ce3d030107034200")!

    /// SPKI DER for an X9.62 uncompressed P-256 point.
    static func p256(x963 point: Data) -> Data? {
        guard point.count == 65, point.first == 0x04 else { return nil }
        return p256Header + point
    }

    /// SHA-256 hex of the SPKI DER of `key`, the pin the pairing payload carries.
    /// SecKeyCopyExternalRepresentation gives the bare point, not SPKI.
    static func pin(of key: SecKey) -> String? {
        guard let point = SecKeyCopyExternalRepresentation(key, nil) as Data?, let spki = p256(x963: point) else {
            return nil
        }
        return spki.sha256Hex
    }

    /// The pin of the leaf certificate in `trust`. Read from the certificate
    /// itself, because SecTrustCopyKey may give nothing for a self-signed
    /// chain that was never evaluated.
    static func pin(of trust: SecTrust) -> String? {
        guard let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate], let leaf = chain.first,
            let key = SecCertificateCopyKey(leaf)
        else { return nil }
        return pin(of: key)
    }
}

/// Accepts the server only if its leaf certificate carries the pinned key.
/// The certificate is self-signed, so the pin replaces chain and host name checks.
final class PinDelegate: NSObject, URLSessionDelegate, Sendable {
    let pin: String

    init(pin: String) {
        self.pin = pin
    }

    func urlSession(
        _ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
            let trust = challenge.protectionSpace.serverTrust
        else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        guard SPKI.pin(of: trust) == pin else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }
}

/// The HTTPS session to the laptop, pinned to its certificate. The app keeps
/// one for the pairing it holds; pairing makes short-lived ones per address.
struct PinnedSession: Sendable {
    enum Patience: Sendable {
        /// Fails fast, so pairing can move on to the next address and try again.
        case pairing
        /// Allows long downloads.
        case standard
    }

    let bases: [URL]
    let session: URLSession
    /// Index into `bases` of the address that last answered.
    private let current = OSAllocatedUnfairLock(initialState: 0)

    init(bases: [URL], pin: String, patience: Patience = .standard) {
        precondition(!bases.isEmpty, "a pinned session needs an address")
        var unique: [URL] = []
        for base in bases where !unique.contains(base) { unique.append(base) }
        self.bases = unique
        let configuration = URLSessionConfiguration.ephemeral
        switch patience {
        case .pairing:
            // Waiting for connectivity would hold a denied or unanswered local network request for the resource timeout.
            configuration.waitsForConnectivity = false
            configuration.timeoutIntervalForRequest = 6
            configuration.timeoutIntervalForResource = 15
        case .standard:
            // Not waiting for connectivity either: a request with no path would otherwise sit for the
            // resource timeout before the next announced address is tried.
            configuration.waitsForConnectivity = false
            configuration.timeoutIntervalForRequest = 30
            configuration.timeoutIntervalForResource = 600
        }
        session = URLSession(configuration: configuration, delegate: PinDelegate(pin: pin), delegateQueue: nil)
    }

    var base: URL { bases[current.withLock { $0 }] }

    func get(_ path: String) async throws -> Data {
        try await fallingBack(idempotent: true) { base in
            try await send(URLRequest(url: url(path, base)))
        }
    }

    /// Posts raw bytes. Only for calls that are safe to repeat on another address.
    func post(_ path: String, data body: Data, type: String) async throws -> Data {
        try await fallingBack(idempotent: true) { base in
            var request = URLRequest(url: url(path, base))
            request.httpMethod = "POST"
            request.setValue(type, forHTTPHeaderField: "content-type")
            request.httpBody = body
            return try await send(request)
        }
    }

    func post(_ path: String, json body: some Encodable) async throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .withoutEscapingSlashes
        let data = try encoder.encode(body)
        return try await fallingBack(idempotent: false) { base in
            var request = URLRequest(url: url(path, base))
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "content-type")
            request.httpBody = data
            return try await send(request)
        }
    }

    /// Streams a large response to a temporary file. Only an error body is read into memory.
    func download(_ path: String) async throws -> URL {
        try await fallingBack(idempotent: true) { base in
            let (file, response) = try await session.download(for: URLRequest(url: url(path, base)))
            do {
                try HTTP.check(response, body: (try? Data(contentsOf: file)) ?? Data(), path: path)
            } catch {
                try? FileManager.default.removeItem(at: file)
                throw error
            }
            return file
        }
    }

    /// Tries the address that last answered, then the others the laptop
    /// announced, so a laptop that changed interfaces is found again. A POST
    /// moves on only when no connection was made, so it is never sent twice.
    private func fallingBack<T: Sendable>(idempotent: Bool, _ operation: (URL) async throws -> T) async throws -> T {
        let first = current.withLock { $0 }
        var last: (any Error)?
        for offset in bases.indices {
            let index = (first + offset) % bases.count
            do {
                let value = try await operation(bases[index])
                current.withLock { $0 = index }
                return value
            } catch let error as URLError where HTTP.neverConnected(error) || (idempotent && HTTP.isConnectionFailure(error)) {
                last = error
            }
        }
        throw last ?? CaptureError("no address for the laptop")
    }

    private func url(_ path: String, _ base: URL) -> URL {
        URL(string: path, relativeTo: base)!.absoluteURL
    }

    private func send(_ request: URLRequest) async throws -> Data {
        let (data, response) = try await session.data(for: request)
        try HTTP.check(response, body: data, path: request.url?.path() ?? "")
        return data
    }
}

/// A refusal from the laptop, with its own explanation when it gave one.
struct HTTPError: LocalizedError {
    let status: Int
    let message: String
    var errorDescription: String? { message }
}

enum HTTP {
    /// `body` is read only for a refusal.
    static func check(_ response: URLResponse, body: @autoclosure () -> Data, path: String) throws {
        guard let http = response as? HTTPURLResponse else { throw CaptureError("\(path) gave no HTTP response") }
        guard (200..<300).contains(http.statusCode) else {
            struct Failure: Decodable { let error: String }
            let message = (try? JSONDecoder().decode(Failure.self, from: body()))?.error
            throw HTTPError(status: http.statusCode, message: message ?? "\(path) failed with \(http.statusCode)")
        }
    }

    static func isConnectionFailure(_ error: URLError) -> Bool {
        neverConnected(error) || [.networkConnectionLost, .timedOut].contains(error.code)
    }

    /// iOS refused the request because Local Network access is off for the app.
    /// URLError carries this only in the description of its network path.
    static func localNetworkDenied(_ error: URLError) -> Bool {
        guard error.code == .notConnectedToInternet, let path = error.userInfo["_NSURLErrorNWPathKey"] else { return false }
        return String(describing: path).localizedCaseInsensitiveContains("local network prohibited")
    }

    /// The request cannot have reached the laptop.
    static func neverConnected(_ error: URLError) -> Bool {
        [.cannotConnectToHost, .notConnectedToInternet, .cannotFindHost].contains(error.code)
    }

    static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        do {
            return try JSONDecoder().decode(type, from: data)
        } catch {
            throw CaptureError("the laptop answered something unexpected: \(error)")
        }
    }
}
