import Foundation

/// The QR payload the presenter page shows, and that `GET /api/v2/pairing` returns.
struct PairingPayload: Codable, Equatable, Sendable {
    static let paramNames = ["proving.key", "circuit.r1cs"]
    /// The port `provenance serve` uses for its TLS listener.
    static let tlsPort = 8792

    let v: Int
    let urls: [String]
    let pin: String
    let token: String
    let params: [String: String]

    /// `requireAddress` is false in the Simulator, which reaches the Mac on localhost.
    static func parse(_ text: String, requireAddress: Bool = true) throws -> PairingPayload {
        let payload: PairingPayload
        do {
            payload = try JSONDecoder().decode(PairingPayload.self, from: Data(text.trimmingCharacters(in: .whitespacesAndNewlines).utf8))
        } catch {
            throw CaptureError("this is not a pairing code from provenance serve")
        }
        try payload.validate(requireAddress: requireAddress)
        return payload
    }

    func validate(requireAddress: Bool = true) throws {
        guard v == 1 else { throw CaptureError("pairing code version \(v) is not supported; update the app") }
        guard HexText.isLowercase(pin, count: 64) else { throw CaptureError("the pairing code has no valid certificate pin") }
        guard HexText.isLowercase(token, count: 32) else { throw CaptureError("the pairing code has no valid token") }
        for name in Self.paramNames where !HexText.isLowercase(params[name] ?? "", count: 64) {
            throw CaptureError("the pairing code has no SHA-256 for \(name)")
        }
        guard !requireAddress || !serverURLs.isEmpty else { throw CaptureError("the pairing code names no https address") }
    }

    var serverURLs: [URL] {
        urls.compactMap(URL.init(string:)).filter { $0.scheme == "https" && $0.host != nil }
    }
}

struct PairRequest: Encodable, Sendable {
    let token: String
    let publicKey: String
    let signature: String
    let kind: String
    let name: String
    /// Left out of the JSON when nil.
    var attestation: AppAttestation? = nil

    enum CodingKeys: String, CodingKey {
        case token, signature, kind, name, attestation
        case publicKey = "public_key"
    }
}

/// What `attestKey` returned, for `POST /api/v2/pair`.
struct AppAttestation: Encodable, Equatable, Sendable {
    /// As `generateKey` returns it, already base64.
    let keyId: String
    /// The attestation object, base64.
    let object: String

    enum CodingKeys: String, CodingKey {
        case object
        case keyId = "key_id"
    }
}

struct Paired: Decodable, Sendable {
    let device: String
    /// The kind as the laptop shows it, such as "attested: secure-enclave (development)".
    var kind: String? = nil
    /// Why the laptop did not check an attestation it got.
    var note: String? = nil
}

struct Region: Decodable, Equatable, Sendable {
    let cell: String
    let resolution: Int
}

struct Outline: Decodable, Equatable, Sendable {
    let cell: String
    let resolution: Int
    let areaKm2: Double
    /// Latitude, longitude pairs in degrees.
    let boundary: [[Double]]

    enum CodingKeys: String, CodingKey {
        case cell, resolution, boundary
        case areaKm2 = "area_km2"
    }
}

/// What `location-proof` takes on stdin. It never leaves the phone.
struct LocationSecrets: Codable, Equatable, Sendable {
    let latitude: Double
    let longitude: Double
    let salt: String
    let hints: [UInt32]

    /// A fresh salt below the BN254 modulus: 31 random bytes behind a zero byte.
    static func freshSalt() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        var generator = SystemRandomNumberGenerator()
        for index in 1..<32 { bytes[index] = UInt8.random(in: .min ... .max, using: &generator) }
        return "0x" + Data(bytes).hex
    }

    func json() throws -> String {
        String(decoding: try JSONEncoder().encode(self), as: UTF8.self)
    }
}

struct LocationProof: Decodable, Equatable, Sendable {
    let envelope: String
    let proof: String
}

struct CaptureUpload: Encodable, Sendable {
    struct Source: Encodable, Sendable {
        let simulated: Bool
        let accessory: Bool
    }

    let originalPng: String
    let receipt: Receipt
    let cell: String
    let resolution: Int
    let crop: CropRect
    /// Applied by the laptop after the crop.
    var turn = Turn.zero
    let accuracyMeters: Double?
    let locationProof: String
    let location: Source

    enum CodingKeys: String, CodingKey {
        case receipt, cell, resolution, crop, turn, location
        case originalPng = "original_png"
        case accuracyMeters = "accuracy_meters"
        case locationProof = "location_proof"
    }

    func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(originalPng, forKey: .originalPng)
        try container.encode(receipt, forKey: .receipt)
        try container.encode(cell, forKey: .cell)
        try container.encode(resolution, forKey: .resolution)
        try container.encode(crop, forKey: .crop)
        // Left out when there is no turn, so a crop alone still uploads to a laptop that predates turns.
        if turn != .zero { try container.encode(turn, forKey: .turn) }
        // The server wants an explicit null, not a missing key.
        try container.encode(accuracyMeters, forKey: .accuracyMeters)
        try container.encode(locationProof, forKey: .locationProof)
        try container.encode(location, forKey: .location)
    }
}

struct Accepted: Decodable, Sendable {
    let id: String
    let dir: String
}

enum Phase: String, Decodable, Sendable {
    case stored, published, queued, publishing, verifying, accepted, rejected, failed

    var isRunning: Bool { self == .queued || self == .publishing || self == .verifying }
}

/// Publish stages and reader checks, as `provenance --json` prints them.
enum Event: Decodable, Equatable, Sendable {
    case started(stage: String)
    case finished(stage: String, detail: String, seconds: Double)
    case checking(check: String, intent: String)
    case passed(check: String, intent: String, outcome: String, seconds: Double)
    case rejected(check: String, intent: String, reason: String, seconds: Double)
    case other

    private enum Keys: String, CodingKey {
        case event, stage, check, intent, detail, outcome, reason, seconds
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        // An event of a kind this app does not know, or with no kind, must not fail the whole status.
        switch try c.decodeIfPresent(String.self, forKey: .event) {
        case "started":
            self = .started(stage: try c.decode(String.self, forKey: .stage))
        case "finished":
            self = .finished(
                stage: try c.decode(String.self, forKey: .stage), detail: try c.decode(String.self, forKey: .detail),
                seconds: try c.decode(Double.self, forKey: .seconds))
        case "checking":
            self = .checking(check: try c.decode(String.self, forKey: .check), intent: try c.decode(String.self, forKey: .intent))
        case "passed":
            self = .passed(
                check: try c.decode(String.self, forKey: .check), intent: try c.decode(String.self, forKey: .intent),
                outcome: try c.decode(String.self, forKey: .outcome), seconds: try c.decode(Double.self, forKey: .seconds))
        case "rejected":
            self = .rejected(
                check: try c.decode(String.self, forKey: .check), intent: try c.decode(String.self, forKey: .intent),
                reason: try c.decode(String.self, forKey: .reason), seconds: try c.decode(Double.self, forKey: .seconds))
        default:
            self = .other
        }
    }
}

struct Claim: Decodable, Equatable, Sendable {
    let device: String
    let capturedAt: String
    let fingerprint: String
    let envelope: String
    let cell: String
    let crop: CropRect

    enum CodingKeys: String, CodingKey {
        case device, fingerprint, envelope, cell, crop
        case capturedAt = "captured_at"
    }
}

struct CaptureStatus: Decodable, Sendable {
    let id: String
    let phase: Phase
    let events: [Event]
    let error: String?
    let claim: Claim?
}

/// What `POST /api/verify` returns: the checks that passed, the first that
/// rejected the file, and the claim when every check passed.
struct Report: Decodable, Sendable {
    struct Step: Decodable, Sendable {
        let check: String
        let detail: String
    }

    struct Result: Decodable, Sendable {
        let passed: [Step]
        let rejected: Step?
        let claim: Claim?
    }

    let verdict: Result
}
