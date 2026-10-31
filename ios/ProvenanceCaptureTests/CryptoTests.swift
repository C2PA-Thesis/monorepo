import CryptoKit
import Security
import XCTest

@testable import ProvenanceCapture

final class CryptoTests: XCTestCase {
    /// Private scalar SHA-256("provenance-capture-test-key"). The expected SPKI
    /// and pin come from OpenSSL, not CryptoKit:
    /// `openssl ec -inform DER -pubout -outform DER | shasum -a 256`.
    private let scalar = Data(hex: "026360a9ce457f39483a81a0c18e5c02111fccf250830c0100c0216cc11637c2")!
    private let opensslSPKI = Data(
        hex:
            "3059301306072a8648ce3d020106082a8648ce3d03010703420004be084f724aa49904233753ecc80c94ff9bdcffd3793abc91b9ee55f6ca1120a9a2a59f68c859b8cdd2450d83261c33c6c478a3a3ec1dd200059fb81530cedb48"
    )!
    private let opensslPin = "fc050319bf9b95d6dbc09513ba6d051d0bcf0955d32160c002c373b117d512e0"

    func testSPKIFromThePointMatchesOpenSSL() throws {
        let key = try P256.Signing.PrivateKey(rawRepresentation: scalar)
        XCTAssertEqual(SPKI.p256(x963: key.publicKey.x963Representation), opensslSPKI)
        XCTAssertEqual(key.publicKey.derRepresentation, opensslSPKI)
        XCTAssertNil(SPKI.p256(x963: Data(count: 64)))
    }

    func testPinOfASecKeyIsTheSPKIHash() throws {
        let point = try P256.Signing.PrivateKey(rawRepresentation: scalar).publicKey.x963Representation
        let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyClass: kSecAttrKeyClassPublic]
        let secKey = try XCTUnwrap(SecKeyCreateWithData(point as CFData, attributes as CFDictionary, nil))
        XCTAssertEqual(SPKI.pin(of: secKey), opensslPin)
    }

    /// A self-signed P-256 certificate like the one `provenance serve` makes, from
    /// `openssl req -new -x509`. Its pin is
    /// `openssl x509 -pubkey -noout | openssl pkey -pubin -outform DER | shasum -a 256`.
    private let selfSigned = Data(base64Encoded: "MIIBTDCB8qADAgECAgkA27/kueBhW1EwCgYIKoZIzj0EAwIwIDEeMBwGA1UEAwwVcHJvdmVuYW5jZSBzZXJ2ZSB0ZXN0MCAXDTI2MTAzMDIyMTIwM1oYDzIxMjYxMDA2MjIxMjAzWjAgMR4wHAYDVQQDDBVwcm92ZW5hbmNlIHNlcnZlIHRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQD5weGIj3uZDKaefpgerNIb/mFc+1E7TCaVDWpXj2eGRbz9Bmm4nx0ZsAviKJpNJo3qKDlvwf0+6wQJTz5eTYOoxMwETAPBgNVHREECDAGhwTAqAEUMAoGCCqGSM49BAMCA0kAMEYCIQCq68795q/9mXKDagB5RdMPqWCXdyZwlrB5yN+zXo0A9QIhAL6RzWp2Xha+IMwtmCp6qoHXK2KdJXz4gEebwRsY+sas")!
    private let selfSignedPin = "66e439c40447d19552c461c007374e31014b00506478183743241d20eb8aaee4"

    func testPinOfAnUnevaluatedSelfSignedTrust() async throws {
        let der = selfSigned
        // Off the main thread, as in PinDelegate: Security warns that building the chain there may block.
        let (before, evaluated, after) = try await Task.detached {
            let certificate = try XCTUnwrap(SecCertificateCreateWithData(nil, der as CFData))
            var trust: SecTrust?
            let policy = SecPolicyCreateSSL(true, "192.168.1.20" as CFString)
            XCTAssertEqual(SecTrustCreateWithCertificates(certificate, policy, &trust), errSecSuccess)
            let unevaluated = try XCTUnwrap(trust)
            return (SPKI.pin(of: unevaluated), SecTrustEvaluateWithError(unevaluated, nil), SPKI.pin(of: unevaluated))
        }.value
        XCTAssertEqual(before, selfSignedPin)
        // The chain fails evaluation, as on the phone, and the pin still reads.
        XCTAssertFalse(evaluated)
        XCTAssertEqual(after, selfSignedPin)
    }

    func testSoftwareKeyIdAndPEM() throws {
        let key = try SoftwareDeviceKey(raw: scalar)
        XCTAssertEqual(key.spki, opensslSPKI)
        XCTAssertEqual(key.id, opensslPin)
        XCTAssertEqual(key.kind, .software)
        let pem = key.pem
        XCTAssertTrue(pem.hasPrefix("-----BEGIN PUBLIC KEY-----\n"))
        XCTAssertTrue(pem.hasSuffix("\n-----END PUBLIC KEY-----\n"))
        let body = pem.split(separator: "\n").dropFirst().dropLast().joined()
        XCTAssertEqual(Data(base64Encoded: body), opensslSPKI)
    }

    func testDERSignatureFromCryptoKitVerifies() throws {
        let key = try SoftwareDeviceKey(raw: scalar)
        let token = Data("0123456789abcdef0123456789abcdef".utf8)
        let der = try key.sign(token)
        XCTAssertEqual(der.first, 0x30)
        let signature = try P256.Signing.ECDSASignature(derRepresentation: der)
        let publicKey = try P256.Signing.PublicKey(derRepresentation: key.spki)
        XCTAssertTrue(publicKey.isValidSignature(signature, for: token))
        XCTAssertFalse(publicKey.isValidSignature(signature, for: Data("other".utf8)))
    }

    func testHex() {
        XCTAssertEqual(Data([0x00, 0xab, 0xff]).hex, "00abff")
        XCTAssertEqual(Data(hex: "00ABff"), Data([0x00, 0xab, 0xff]))
        XCTAssertNil(Data(hex: "abc"))
        XCTAssertNil(Data(hex: "zz"))
    }

    func testFreshSaltIsBelowTheModulus() {
        let salt = LocationSecrets.freshSalt()
        XCTAssertTrue(salt.hasPrefix("0x00"))
        XCTAssertTrue(HexText.isLowercase(String(salt.dropFirst(2)), count: 64))
    }
}
