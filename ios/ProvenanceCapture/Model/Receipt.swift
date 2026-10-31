import Foundation

/// The lattice fingerprint of the 1024x512 original, as crates/fingerprint serializes it.
struct Fingerprint: Codable, Equatable, Sendable {
    static let algorithm = "hyperveritas-ajtai-chacha8-v1"
    static let rows = 128
    static let elementBytes = 32

    struct Size: Codable, Equatable, Sendable {
        var width: Int
        var height: Int
    }

    var algorithm: String
    var size: Size
    var r: [String]
    var g: [String]
    var b: [String]

    enum CodingKeys: String, CodingKey {
        case algorithm, size
        case r = "R"
        case g = "G"
        case b = "B"
    }

    /// From the binary form: R, G, then B, 128 big-endian 32-byte elements each.
    init(binary: Data) throws {
        let perChannel = Self.rows * Self.elementBytes
        guard binary.count == 3 * perChannel else {
            throw CaptureError("the fingerprint has \(binary.count) bytes, not \(3 * perChannel)")
        }
        let bytes = [UInt8](binary)
        let channel = { (index: Int) -> [String] in
            (0..<Self.rows).map { row in
                let start = index * perChannel + row * Self.elementBytes
                return "0x" + Data(bytes[start..<start + Self.elementBytes]).hex
            }
        }
        self.init(
            algorithm: Self.algorithm,
            size: Size(width: Original.width, height: Original.height),
            r: channel(0), g: channel(1), b: channel(2))
    }

    init(algorithm: String, size: Size, r: [String], g: [String], b: [String]) {
        self.algorithm = algorithm
        self.size = size
        self.r = r
        self.g = g
        self.b = b
    }
}

/// The receipt as crates/provenance/src/receipt.rs defines it.
struct Receipt: Codable, Equatable, Sendable {
    var fingerprint: Fingerprint
    var envelope: String
    var capturedAt: String
    var device: String
    var signature: String

    enum CodingKeys: String, CodingKey {
        case fingerprint, envelope, device, signature
        case capturedAt = "captured_at"
    }

    static func sign(
        fingerprint: Fingerprint, envelope: String, capturedAt: String, key: any DeviceKey
    ) throws -> Receipt {
        let bytes = signedBytes(fingerprint: fingerprint, envelope: envelope, capturedAt: capturedAt, device: key.id)
        let signature = try key.sign(bytes)
        return Receipt(
            fingerprint: fingerprint, envelope: envelope, capturedAt: capturedAt, device: key.id,
            signature: signature.base64EncodedString())
    }

    /// The compact JSON of the four signed fields in the Rust field order. It
    /// is written by hand because JSONEncoder does not promise a key order.
    static func signedBytes(fingerprint: Fingerprint, envelope: String, capturedAt: String, device: String) -> Data {
        let list = { (values: [String]) in "[" + values.map(jsonString).joined(separator: ",") + "]" }
        var json = "{\"fingerprint\":{"
        json += "\"algorithm\":" + jsonString(fingerprint.algorithm)
        json += ",\"size\":{\"width\":\(fingerprint.size.width),\"height\":\(fingerprint.size.height)}"
        json += ",\"R\":" + list(fingerprint.r)
        json += ",\"G\":" + list(fingerprint.g)
        json += ",\"B\":" + list(fingerprint.b)
        json += "},\"envelope\":" + jsonString(envelope)
        json += ",\"captured_at\":" + jsonString(capturedAt)
        json += ",\"device\":" + jsonString(device)
        json += "}"
        return Data(json.utf8)
    }
}

enum Timestamp {
    /// RFC 3339 in UTC with whole seconds, as the Rust side and the web page write it.
    static func rfc3339(_ date: Date) -> String {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let whole = Date(timeIntervalSince1970: date.timeIntervalSince1970.rounded(.down))
        let parts = calendar.dateComponents([.year, .month, .day, .hour, .minute, .second], from: whole)
        return String(
            format: "%04d-%02d-%02dT%02d:%02d:%02dZ",
            parts.year!, parts.month!, parts.day!, parts.hour!, parts.minute!, parts.second!)
    }
}

/// An error whose message is meant for the screen.
struct CaptureError: LocalizedError, Equatable {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

func describe(_ error: any Error) -> String {
    if let error = error as? LocalizedError, let text = error.errorDescription { return text }
    return error.localizedDescription
}
