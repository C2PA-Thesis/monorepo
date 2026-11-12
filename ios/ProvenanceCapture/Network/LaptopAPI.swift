import Foundation

/// The calls `provenance serve` answers on its TLS listener. See crates/provenance/src/serve.rs.
struct LaptopAPI: Sendable {
    let http: PinnedSession

    /// `bases` are tried in order when a connection fails, starting with the last one that answered.
    init(bases: [URL], pin: String, patience: PinnedSession.Patience = .standard) {
        http = PinnedSession(bases: bases, pin: pin, patience: patience)
    }

    /// Ends the session once its requests finish. URLSession keeps its delegate until then.
    func invalidate() {
        http.session.finishTasksAndInvalidate()
    }

    func pair(_ request: PairRequest) async throws -> Paired {
        try HTTP.decode(Paired.self, try await http.post("/api/v2/pair", json: request))
    }

    func server() async throws -> ServerInfo {
        try HTTP.decode(ServerInfo.self, try await http.get("/api/v2/server"))
    }

    func challenge() async throws -> RegistrationChallenge {
        try HTTP.decode(RegistrationChallenge.self, try await http.post("/api/v2/challenge", data: Data(), type: "application/json"))
    }

    func register(_ request: RegisterRequest) async throws -> Paired {
        try HTTP.decode(Paired.self, try await http.post("/api/v2/register", json: request))
    }

    /// Whether the laptop trusts the key with this id. Carries no secret, so it is safe to repeat.
    func trusted(_ device: String) async throws -> Bool {
        struct Trust: Decodable { let trusted: Bool }
        return try HTTP.decode(Trust.self, try await http.get("/api/devices/\(device)")).trusted
    }

    /// Only for the published cell: the outline of a cell the user only previewed would tell the laptop more than the file does.
    func outline(_ cell: String) async throws -> Outline {
        try HTTP.decode(Outline.self, try await http.get("/api/cells/\(cell)"))
    }

    func upload(_ upload: CaptureUpload) async throws -> Accepted {
        try HTTP.decode(Accepted.self, try await http.post("/api/v2/captures", json: upload))
    }

    func status(_ id: String) async throws -> CaptureStatus {
        try HTTP.decode(CaptureStatus.self, try await http.get("/api/captures/\(id)"))
    }

    func signedPNG(_ id: String) async throws -> Data {
        try await http.get("/api/captures/\(id)/signed.png")
    }

    /// Runs the laptop's reader checks on a file. A published photo carries no
    /// more than what anyone it is shared with already sees.
    func verify(_ file: Data) async throws -> Report {
        try HTTP.decode(Report.self, try await http.post("/api/verify", data: file, type: "image/png"))
    }

    func param(_ name: String) async throws -> URL {
        try await http.download("/api/params/location/\(name)")
    }
}

/// The Simulator reaches the Mac's localhost, where `provenance serve` hands
/// the pairing payload to local callers only.
enum LocalPairing {
    static let base = URL(string: "http://127.0.0.1:8791")!

    static func fetch() async throws -> PairingPayload {
        let payload: PairingPayload
        do {
            payload = try HTTP.decode(PairingPayload.self, try await get("/api/v2/pairing"))
        } catch let error as HTTPError where error.status == 404 {
            // The presenter page's route wraps the same payload as {"qr": ...}.
            struct Session: Decodable { let qr: PairingPayload }
            payload = try HTTP.decode(Session.self, try await get("/api/v2/session")).qr
        }
        try payload.validate(requireAddress: false)
        return payload
    }

    private static func get(_ path: String) async throws -> Data {
        let (data, response) = try await URLSession.shared.data(from: base.appending(path: path))
        try HTTP.check(response, body: data, path: path)
        return data
    }
}

/// What pairing leaves behind, kept in UserDefaults. The token is not kept: it is single use.
struct Pairing: Codable, Equatable, Sendable {
    /// The address that answered the pairing request.
    var base: URL
    /// Every address to fall back on, in the order pairing tried them.
    var urls: [URL]
    var pin: String
    var device: String
    var kind: DeviceKeyKind
    /// The kind as the laptop recorded it. Nil for pairings saved before it was kept.
    var shownKind: String? = nil
    /// Why the key paired without an App Attest attestation, when it did.
    var attestNote: String? = nil
    /// The known server this phone registered with, or nil for a laptop it paired with.
    var server: String? = nil
    var params: [String: String]
    var pairedAt: Date
    var paramsReady: Bool

    private static let key = "pairing.v1"

    static func load() -> Pairing? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(Pairing.self, from: data)
    }

    func save() {
        if let data = try? JSONEncoder().encode(self) { UserDefaults.standard.set(data, forKey: Self.key) }
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: key)
    }
}
