import CryptoKit
import XCTest

@testable import ProvenanceCapture

final class ReceiptTests: XCTestCase {
    /// The fixture is JSON.stringify of the same values, exactly as
    /// web/src/receipt.ts signedBytes builds them (Fixtures/receipt-signed-bytes.json).
    func testSignedBytesMatchTheWebAndRustFormat() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "receipt-signed-bytes", withExtension: "json"))
        let expected = try Data(contentsOf: url)
        let digest = { (text: String) in Data(SHA256.hash(data: Data(text.utf8))).hex }
        let channel = { (name: String) in (0..<128).map { "0x" + digest("\(name)\($0)") } }
        let fingerprint = Fingerprint(
            algorithm: Fingerprint.algorithm, size: .init(width: 1024, height: 512), r: channel("R"), g: channel("G"),
            b: channel("B"))
        let bytes = Receipt.signedBytes(
            fingerprint: fingerprint, envelope: "0x" + digest("envelope"), capturedAt: "2026-10-30T18:04:05Z",
            device: digest("device"))
        XCTAssertEqual(bytes.count, 26803)
        XCTAssertEqual(bytes, expected)
    }

    func testSignedBytesKeyOrderAndSpacing() {
        let fingerprint = Fingerprint(
            algorithm: "hyperveritas-ajtai-chacha8-v1", size: .init(width: 1024, height: 512), r: ["0x01", "0x02"],
            g: ["0x03"], b: [])
        let bytes = Receipt.signedBytes(fingerprint: fingerprint, envelope: "0xab", capturedAt: "2024-01-02T03:04:05Z", device: "cd")
        XCTAssertEqual(
            String(decoding: bytes, as: UTF8.self),
            #"{"fingerprint":{"algorithm":"hyperveritas-ajtai-chacha8-v1","size":{"width":1024,"height":512},"R":["0x01","0x02"],"G":["0x03"],"B":[]},"envelope":"0xab","captured_at":"2024-01-02T03:04:05Z","device":"cd"}"#
        )
    }

    /// The same escapes JSON.stringify and serde_json write.
    func testJSONStringEscapes() {
        XCTAssertEqual(jsonString("a\"b\\c\nd\u{01}é"), #""a\"b\\c\nd\u0001é""#)
    }

    func testTimestampIsUTCWithWholeSeconds() {
        XCTAssertEqual(Timestamp.rfc3339(Date(timeIntervalSince1970: 1_700_000_000.999)), "2023-11-14T22:13:20Z")
        XCTAssertEqual(Timestamp.rfc3339(Date(timeIntervalSince1970: 0)), "1970-01-01T00:00:00Z")
        XCTAssertEqual(Timestamp.rfc3339(Date(timeIntervalSince1970: 951_782_400)), "2000-02-29T00:00:00Z")
    }

    func testFingerprintFromBinaryKeepsChannelAndRowOrder() throws {
        var binary = Data(repeating: 0, count: 3 * 128 * 32)
        binary[31] = 0x01  // R row 0, last byte
        binary[128 * 32 + 32] = 0xff  // G row 1, first byte
        binary[2 * 128 * 32 + 127 * 32 + 31] = 0x07  // B row 127, last byte
        let fingerprint = try Fingerprint(binary: binary)
        XCTAssertEqual(fingerprint.r[0], "0x" + String(repeating: "0", count: 63) + "1")
        XCTAssertEqual(fingerprint.g[1], "0xff" + String(repeating: "0", count: 62))
        XCTAssertEqual(fingerprint.b[127], "0x" + String(repeating: "0", count: 63) + "7")
        XCTAssertEqual(fingerprint.size, .init(width: 1024, height: 512))
        XCTAssertThrowsError(try Fingerprint(binary: Data(count: 10)))
    }

    func testReceiptSignatureVerifiesOverTheSignedBytes() throws {
        let key = SoftwareDeviceKey.create()
        let fingerprint = Fingerprint(
            algorithm: Fingerprint.algorithm, size: .init(width: 1024, height: 512), r: ["0x01"], g: ["0x02"], b: ["0x03"])
        let receipt = try Receipt.sign(fingerprint: fingerprint, envelope: "0x05", capturedAt: "2026-01-01T00:00:00Z", key: key)
        XCTAssertEqual(receipt.device, key.id)
        let signature = try P256.Signing.ECDSASignature(derRepresentation: XCTUnwrap(Data(base64Encoded: receipt.signature)))
        let publicKey = try P256.Signing.PublicKey(derRepresentation: key.spki)
        let bytes = Receipt.signedBytes(
            fingerprint: fingerprint, envelope: receipt.envelope, capturedAt: receipt.capturedAt, device: receipt.device)
        XCTAssertTrue(publicKey.isValidSignature(signature, for: bytes))
        XCTAssertFalse(publicKey.isValidSignature(signature, for: bytes + Data([0])))
    }
}
