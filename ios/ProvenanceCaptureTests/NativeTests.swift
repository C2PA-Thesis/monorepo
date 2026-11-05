import CryptoKit
import XCTest

@testable import ProvenanceCapture

/// The linked Rust and Go libraries, through the same bridge the capture uses,
/// against vectors pinned on the host side.
final class NativeTests: XCTestCase {
    /// crates/fingerprint/src/lib.rs: SHA-256 of the 384 elements of the test
    /// image's fingerprint, R then G then B, in binary, at 1024x512
    /// (FINGERPRINT_MAX_PIXELS) and at 836x627, the original of a 4:3 photo.
    private let testImageDigest = "cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083"
    private let fullFrameDigest = "32ae4e96e4679d0dad3e1b10dde31f46d18d15ac25606f73b0f4a014117ab375"

    /// location-proof/main_test.go: the UTDT demo coordinate, salt 1, its
    /// hints, envelope and resolution 7 cell.
    private let latitude = -34.5478
    private let longitude = -58.4462
    private let saltOne = "0x" + String(repeating: "0", count: 63) + "1"
    private let demoHints: [UInt32] = [
        3_198_105_316, 3_205_573_900, 3_197_634_690, 1_064_596_499, 3_205_444_996, 3_210_356_339, 3_204_052_702, 1_063_217_901,
    ]
    private let demoEnvelope = "0x0c5fe416c2846000ee1e623e19158db8cb35dbb69617a5c99ea523839e75ed29"
    private let demoCell = "87c2e3020ffffff"

    func testBothLibrariesAreLinked() {
        XCTAssertTrue(Native.fingerprintLinked, "Fingerprint.xcframework is not linked; run ios/scripts/build-rust.sh")
        XCTAssertTrue(Native.zklocLinked, "ZKLoc.xcframework is not linked; run ios/scripts/build-go.sh")
    }

    func testMaxPixelsMatchesTheHeader() {
        XCTAssertEqual(Native.headerMaxPixels, OriginalSize.maxPixels)
    }

    func testFingerprintOfTheTestImageMatchesThePinnedDigest() throws {
        let size = OriginalSize(width: 1024, height: 512)
        XCTAssertEqual(size.pixels, OriginalSize.maxPixels)
        XCTAssertEqual(try digest(size), testImageDigest)
    }

    func testFingerprintOfTheFullFrameTestImageMatchesThePinnedDigest() throws {
        XCTAssertEqual(try digest(OriginalSize(width: 836, height: 627)), fullFrameDigest)
    }

    func testFingerprintRefusesPlanesOfAnotherSize() throws {
        let planes = try Native.testImage(OriginalSize(width: 836, height: 627))
        XCTAssertThrowsError(
            try Native.library.fingerprint(r: planes.r, g: planes.g, b: planes.b, size: OriginalSize(width: 1024, height: 512)))
        XCTAssertThrowsError(
            try Native.library.fingerprint(r: [], g: [], b: [], size: OriginalSize(width: 0, height: 0)))
    }

    private func digest(_ size: OriginalSize) throws -> String {
        let planes = try Native.testImage(size)
        let fingerprint = try Native.library.fingerprint(r: planes.r, g: planes.g, b: planes.b, size: size)
        XCTAssertEqual(fingerprint.size, .init(width: size.width, height: size.height))
        XCTAssertEqual(fingerprint.r.count, Fingerprint.rows)
        let binary = (fingerprint.r + fingerprint.g + fingerprint.b).reduce(into: Data()) { data, element in
            data.append(Data(hex: String(element.dropFirst(2)))!)
        }
        XCTAssertEqual(binary.count, 3 * Fingerprint.rows * Fingerprint.elementBytes)
        return Data(SHA256.hash(data: binary)).hex
    }

    func testHintsOfTheDemoCoordinateMatchGo() throws {
        XCTAssertEqual(try Native.library.hints(latitude: latitude, longitude: longitude), demoHints)
    }

    func testEnvelopeOfTheDemoCoordinateMatchesGo() throws {
        let hints = try Native.library.hints(latitude: latitude, longitude: longitude)
        let secrets = LocationSecrets(latitude: latitude, longitude: longitude, salt: saltOne, hints: hints)
        XCTAssertEqual(try Native.library.envelope(secrets), demoEnvelope)
    }

    func testEnvelopeRefusesHintsOfAnotherCoordinate() throws {
        let tokyo = try Native.library.hints(latitude: 35.6762, longitude: 139.6503)
        let secrets = LocationSecrets(latitude: latitude, longitude: longitude, salt: saltOne, hints: tokyo)
        XCTAssertThrowsError(try Native.library.envelope(secrets))
    }

    /// Opt in with params from `location-proof setup`, which CI does not make:
    /// `TEST_RUNNER_ZKLOC_PARAMS=<dir> xcodebuild test ...`. With
    /// `TEST_RUNNER_ZKLOC_PROOF_OUT=<file>` the proof is written there, so the
    /// host can check it with `location-proof verify`.
    func testProveTheDemoCoordinateWhenParamsAreGiven() throws {
        guard let dir = ProcessInfo.processInfo.environment["ZKLOC_PARAMS"] else {
            throw XCTSkip("set TEST_RUNNER_ZKLOC_PARAMS to a location-proof params directory")
        }
        let hints = try Native.library.hints(latitude: latitude, longitude: longitude)
        let secrets = LocationSecrets(latitude: latitude, longitude: longitude, salt: saltOne, hints: hints)
        let proof = try Native.library.prove(params: URL(filePath: dir), cell: demoCell, secrets: secrets)
        XCTAssertEqual(proof.envelope, demoEnvelope)
        XCTAssertNotNil(Data(base64Encoded: proof.proof))
        if let out = ProcessInfo.processInfo.environment["ZKLOC_PROOF_OUT"] {
            try Data(proof.proof.utf8).write(to: URL(filePath: out))
        }
    }

    func testCellOfTheDemoCoordinateMatchesGo() throws {
        let region = try Native.library.cell(latitude: latitude, longitude: longitude, resolution: 7)
        XCTAssertEqual(region, Region(cell: demoCell, resolution: 7))
    }
}
