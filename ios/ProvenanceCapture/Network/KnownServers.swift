import Foundation

/// A server this build registers with, without a QR code. Its address and the
/// pin of its TLS key come from `servers.json`, which the deploy fills in.
struct KnownServer: Codable, Equatable, Sendable, Identifiable {
    let name: String
    let urls: [String]
    let pin: String

    var id: String { name }

    var serverURLs: [URL] {
        urls.compactMap(URL.init(string:)).filter { $0.scheme == "https" && $0.host != nil }
    }

    /// An entry the deploy has not filled in yet is left out.
    var isUsable: Bool {
        !name.isEmpty && HexText.isLowercase(pin, count: 64) && !serverURLs.isEmpty
    }
}

enum KnownServers {
    private struct File: Decodable { let servers: [KnownServer] }

    static let all: [KnownServer] = load(from: Bundle.main.url(forResource: "servers", withExtension: "json"))

    static func load(from url: URL?) -> [KnownServer] {
        guard let url, let data = try? Data(contentsOf: url) else { return [] }
        return parse(data)
    }

    static func parse(_ data: Data) -> [KnownServer] {
        ((try? JSONDecoder().decode(File.self, from: data))?.servers ?? []).filter(\.isUsable)
    }

    /// Registration needs App Attest, which the Simulator does not have.
    static var available: [KnownServer] {
        #if targetEnvironment(simulator)
            []
        #else
            all
        #endif
    }
}
