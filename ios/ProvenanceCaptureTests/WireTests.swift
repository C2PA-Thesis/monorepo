import XCTest

@testable import ProvenanceCapture

final class WireTests: XCTestCase {
    private let pin = String(repeating: "ab", count: 32)
    private let paramHash = String(repeating: "cd", count: 32)

    private func payload(token: String = String(repeating: "0f", count: 16), urls: String = #"["https://192.168.1.20:8792"]"#) -> String {
        #"{"v":1,"urls":\#(urls),"pin":"\#(pin)","token":"\#(token)","params":{"proving.key":"\#(paramHash)","circuit.r1cs":"\#(paramHash)"}}"#
    }

    func testParsesThePairingPayload() throws {
        let parsed = try PairingPayload.parse(payload())
        XCTAssertEqual(parsed.serverURLs, [URL(string: "https://192.168.1.20:8792")!])
        XCTAssertEqual(parsed.pin, pin)
        XCTAssertEqual(parsed.params["circuit.r1cs"], paramHash)
    }

    /// Pinned in crates/provenance/src/serve.rs too (`client_data_is_fixed`).
    func testTheAttestedClientDataMatchesTheLaptop() {
        let hash = PairingAttestation.clientDataHash(token: "0123456789abcdef0123456789abcdef", device: String(repeating: "ab", count: 32))
        XCTAssertEqual(hash.hex, "16506113a77c759bbec2f3084dd656b07268fbf4f5df890834bf3aa7f92981bc")
    }

    func testThePairRequestCarriesTheAttestationOnlyWhenThereIsOne() throws {
        var request = PairRequest(token: "t", publicKey: "k", signature: "s", kind: "secure-enclave", name: "n")
        let bare = try JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as! [String: Any]
        XCTAssertEqual(Set(bare.keys), ["token", "public_key", "signature", "kind", "name"])
        request.attestation = AppAttestation(keyId: "a2V5", object: "b2JqZWN0")
        let attested = try JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as! [String: Any]
        XCTAssertEqual(attested["attestation"] as? [String: String], ["key_id": "a2V5", "object": "b2JqZWN0"])
    }

    func testReadsWhatTheLaptopSaysAboutTheKey() throws {
        let old = try JSONDecoder().decode(Paired.self, from: Data(#"{"device":"ab"}"#.utf8))
        XCTAssertNil(old.kind)
        let attested = try JSONDecoder().decode(Paired.self, from: Data(#"{"device":"ab","kind":"attested: secure-enclave (development)"}"#.utf8))
        XCTAssertEqual(attested.kind, "attested: secure-enclave (development)")
    }

    func testRejectsBadPairingPayloads() {
        XCTAssertThrowsError(try PairingPayload.parse("hello"))
        XCTAssertThrowsError(try PairingPayload.parse(payload(token: "short")))
        XCTAssertThrowsError(try PairingPayload.parse(payload(urls: #"["http://192.168.1.20:8791"]"#)))
    }

    func testTheSimulatorAcceptsAPayloadWithNoLANAddress() throws {
        let parsed = try PairingPayload.parse(payload(urls: "[]"), requireAddress: false)
        XCTAssertTrue(parsed.serverURLs.isEmpty)
        XCTAssertThrowsError(try PairingPayload.parse(payload(urls: "[]")))
    }

    func testOnlyConnectFailuresCountAsNeverConnected() {
        XCTAssertTrue(HTTP.neverConnected(URLError(.cannotConnectToHost)))
        XCTAssertFalse(HTTP.neverConnected(URLError(.timedOut)))
        XCTAssertTrue(HTTP.isConnectionFailure(URLError(.timedOut)))
        XCTAssertFalse(HTTP.isConnectionFailure(URLError(.cancelled)))
    }

    /// A downloaded proving key is tens of MB, so a 200 must not read the file into memory.
    func testCheckReadsTheBodyOnlyForARefusal() throws {
        let url = URL(string: "https://192.168.1.20:8792/api/params/location/proving.key")!
        var reads = 0
        func body() -> Data {
            reads += 1
            return Data(#"{"error":"no such file"}"#.utf8)
        }
        let ok = HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: nil)!
        try HTTP.check(ok, body: body(), path: url.path())
        XCTAssertEqual(reads, 0)
        let missing = HTTPURLResponse(url: url, statusCode: 404, httpVersion: nil, headerFields: nil)!
        XCTAssertThrowsError(try HTTP.check(missing, body: body(), path: url.path())) { error in
            XCTAssertEqual((error as? HTTPError)?.message, "no such file")
        }
        XCTAssertEqual(reads, 1)
    }

    /// The path text iOS attaches when Local Network access is off for the app.
    func testRecognisesALocalNetworkRefusal() {
        let refused = URLError(.notConnectedToInternet, userInfo: ["_NSURLErrorNWPathKey": "unsatisfied (Local network prohibited), interface: en0[802.11]"])
        XCTAssertTrue(HTTP.localNetworkDenied(refused))
        XCTAssertFalse(HTTP.localNetworkDenied(URLError(.notConnectedToInternet)))
        XCTAssertFalse(HTTP.localNetworkDenied(URLError(.cannotConnectToHost, userInfo: refused.userInfo)))
    }

    func testUploadSendsNullAccuracyAndSnakeCaseKeys() throws {
        let receipt = Receipt(
            fingerprint: Fingerprint(algorithm: "a", size: .init(width: 1, height: 1), r: [], g: [], b: []),
            envelope: "0x01", capturedAt: "t", device: "d", signature: "s")
        let upload = CaptureUpload(
            originalPng: "png", receipt: receipt, cell: "87", resolution: 7, crop: CropRect(x: 0, y: 0, width: 512, height: 512), accuracyMeters: nil,
            locationProof: "proof", location: .init(simulated: true, accessory: false))
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(upload)) as? [String: Any])
        XCTAssertTrue(object["accuracy_meters"] is NSNull)
        XCTAssertEqual(object["location_proof"] as? String, "proof")
        XCTAssertNil(object["secrets"])
        let inner = try XCTUnwrap(object["receipt"] as? [String: Any])
        XCTAssertEqual(inner["captured_at"] as? String, "t")
        XCTAssertNotNil((inner["fingerprint"] as? [String: Any])?["R"])
    }

    func testSecretsCarryTheHints() throws {
        let secrets = LocationSecrets(latitude: -34.6037, longitude: -58.3816, salt: "0x00", hints: [1, 2, 3, 4, 5, 6, 7, 8])
        let decoded = try JSONDecoder().decode(LocationSecrets.self, from: Data(secrets.json().utf8))
        XCTAssertEqual(decoded, secrets)
    }

    func testStatusEventsBuildTheTimeline() throws {
        let json = #"""
            {"id":"x","phase":"failed","events":[
              {"event":"started","stage":"crop"},
              {"event":"finished","stage":"crop","detail":"512×512","seconds":0.2},
              {"event":"started","stage":"location-proof"}
            ],"error":"boom","claim":null}
            """#
        let status = try JSONDecoder().decode(CaptureStatus.self, from: Data(json.utf8))
        let groups = Timeline.groups(status.events, halted: status.error)
        XCTAssertEqual(groups.count, 2)
        XCTAssertEqual(groups[0].items[0].state, .done(seconds: 0.2, detail: "512×512", note: nil))
        XCTAssertEqual(groups[0].items[1].state, .failed(seconds: nil, detail: "boom", note: nil))
        XCTAssertEqual(groups[0].items[2].state, .skipped)
        XCTAssertEqual(groups[1].items[0].state, .skipped)
        XCTAssertEqual(groups[0].seconds, 0.2, accuracy: 1e-9)
        XCTAssertEqual(groups[1].seconds, 0)
    }

    func testRowTimesUnderFiftyMillisecondsAreOmitted() {
        XCTAssertNil(Verdict.rowTime(0.049))
        XCTAssertEqual(Verdict.rowTime(0.05), "0.1 s")
        XCTAssertEqual(Verdict.rowTime(3.62), "3.6 s")
    }

    func testAreaAndResolutionText() {
        XCTAssertEqual(Verdict.area(252.904), "253 km²")
        XCTAssertEqual(Verdict.area(1770.348), "1,770 km²")
        XCTAssertEqual(Verdict.area(5.161), "5.2 km²")
        XCTAssertEqual(Verdict.area(0.737), "73.7 ha")
        XCTAssertEqual(Verdict.area(0.00215), "2,150 m²")
        XCTAssertEqual(Verdict.resolutionText(7), "~5.2 km² · a neighbourhood")
        XCTAssertEqual(Verdict.cellShort("87c2e3062ffffff"), "87c2e3062…")
        XCTAssertEqual(Verdict.cellShort("87c2e3062"), "87c2e3062")
        XCTAssertEqual(Verdict.resolution(of: "87c2e3062ffffff"), 7)
        XCTAssertEqual(Verdict.resolution(of: "8928308280fffff"), 9)
        XCTAssertEqual(Verdict.area(of: "87c2e3062ffffff", outline: nil), "~5.2 km²")
        XCTAssertEqual(Verdict.short("0x0123456789abcdef"), "0x0123…cdef")
        XCTAssertEqual(Verdict.short("abcd"), "abcd")
    }
}
