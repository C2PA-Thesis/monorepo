import Foundation

#if canImport(CropProofFFI)
    import CropProofFFI
#endif
#if canImport(FingerprintFFI)
    import FingerprintFFI
#endif
#if canImport(ZKLocFFI)
    import ZKLocFFI
#endif

/// Everything the app needs from the Rust fingerprint and the Go location
/// circuit. The rest of the app only sees this protocol.
protocol NativeLibrary: Sendable {
    /// The fingerprint of the original's planes, `size.pixels` bytes each.
    func fingerprint(r: [UInt8], g: [UInt8], b: [UInt8], size: OriginalSize) throws -> Fingerprint
    /// The eight float32 trig hints the circuit takes for the coordinate.
    func hints(latitude: Double, longitude: Double) throws -> [UInt32]
    /// The MiMC envelope over the coordinate, salt and hints.
    func envelope(_ secrets: LocationSecrets) throws -> String
    /// A Groth16 proof that the sealed hints map to `cell`. The circuit does not derive the hints from the coordinate.
    func prove(params: URL, cell: String, secrets: LocationSecrets) throws -> LocationProof
    /// The H3 cell of the coordinate, as the circuit computes it.
    func cell(latitude: Double, longitude: Double, resolution: Int) throws -> Region
    /// The corners and area of `cell`, computed on the phone.
    func outline(_ cell: String) throws -> Outline
}

enum NativeError: LocalizedError {
    case notBuilt(String)
    case failed(String, String)

    var errorDescription: String? {
        switch self {
        case .notBuilt(let library):
            "the native library \(library) is not built into this app; see ios/README.md"
        case .failed(let call, let message):
            "\(call): \(message)"
        }
    }
}

enum Native {
    static let library: any NativeLibrary = LinkedLibrary()

    static var zklocLinked: Bool {
        #if canImport(ZKLocFFI)
            true
        #else
            false
        #endif
    }

    static var fingerprintLinked: Bool {
        #if canImport(FingerprintFFI)
            true
        #else
            false
        #endif
    }

    static var cropProofLinked: Bool {
        #if canImport(CropProofFFI)
            true
        #else
            false
        #endif
    }

    #if DEBUG
        /// FINGERPRINT_MAX_PIXELS, so the tests can check `OriginalSize.maxPixels` against it.
        static var headerMaxPixels: Int? {
            #if canImport(FingerprintFFI)
                Int(FINGERPRINT_MAX_PIXELS)
            #else
                nil
            #endif
        }

        /// Whether Rust's large allocations get a mapping of their own in this
        /// app, as the allocator of crates/crop-proof-ffi gives them. False
        /// means the linker took the allocator of another Rust library: the
        /// app must link ProvenanceRust.xcframework and no other Rust archive.
        static var rustMapsLargeAllocations: Bool? {
            #if canImport(CropProofFFI)
                crop_proof_large_allocations_are_mapped()
            #else
                nil
            #endif
        }

        /// The deterministic image whose fingerprint digests crates/fingerprint pins,
        /// so the tests can run the linked library on a known input.
        static func testImage(_ size: OriginalSize) throws -> (r: [UInt8], g: [UInt8], b: [UInt8]) {
            #if canImport(FingerprintFFI)
                var r = [UInt8](repeating: 0, count: size.pixels)
                var g = r
                var b = r
                let status = r.withUnsafeMutableBufferPointer { r in
                    g.withUnsafeMutableBufferPointer { g in
                        b.withUnsafeMutableBufferPointer { b in
                            fingerprint_test_image(r.baseAddress!, g.baseAddress!, b.baseAddress!, r.count)
                        }
                    }
                }
                guard status == 0 else { throw NativeError.failed("fingerprint_test_image", "status \(status)") }
                return (r, g, b)
            #else
                throw NativeError.notBuilt("ProvenanceRust.xcframework")
            #endif
        }
    #endif
}

/// The XCFrameworks from ios/scripts/build-rust.sh and build-go.sh, when linked.
private struct LinkedLibrary: NativeLibrary {
    func fingerprint(r: [UInt8], g: [UInt8], b: [UInt8], size: OriginalSize) throws -> Fingerprint {
        try checkPlanes(r: r, g: g, b: b, size: size)
        #if canImport(FingerprintFFI)
            var out = [UInt8](repeating: 0, count: 3 * Fingerprint.rows * Fingerprint.elementBytes)
            let status = r.withUnsafeBufferPointer { r in
                g.withUnsafeBufferPointer { g in
                    b.withUnsafeBufferPointer { b in
                        out.withUnsafeMutableBufferPointer { out in
                            fingerprint_rgb(r.baseAddress!, g.baseAddress!, b.baseAddress!, r.count, out.baseAddress!)
                        }
                    }
                }
            }
            guard status == 0 else { throw NativeError.failed("fingerprint_rgb", "status \(status)") }
            return try Fingerprint(binary: Data(out), size: size)
        #else
            throw NativeError.notBuilt("ProvenanceRust.xcframework")
        #endif
    }

    func hints(latitude: Double, longitude: Double) throws -> [UInt32] {
        struct Reply: Decodable { let hints: [UInt32] }
        #if canImport(ZKLocFFI)
            let reply = try Self.call("zkloc_hints", Reply.self) { zkloc_hints(latitude, longitude) }
            guard reply.hints.count == 8 else { throw NativeError.failed("zkloc_hints", "expected 8 hints") }
            return reply.hints
        #else
            throw NativeError.notBuilt("ZKLoc.xcframework")
        #endif
    }

    func envelope(_ secrets: LocationSecrets) throws -> String {
        struct Reply: Decodable { let envelope: String }
        #if canImport(ZKLocFFI)
            let json = try secrets.json()
            return try Self.call("zkloc_envelope", Reply.self) {
                Self.withCString(json) { zkloc_envelope($0) }
            }.envelope
        #else
            throw NativeError.notBuilt("ZKLoc.xcframework")
        #endif
    }

    func prove(params: URL, cell: String, secrets: LocationSecrets) throws -> LocationProof {
        #if canImport(ZKLocFFI)
            let json = try secrets.json()
            let dir = params.path(percentEncoded: false)
            return try Self.call("zkloc_prove", LocationProof.self) {
                Self.withCString(dir) { dir in
                    Self.withCString(cell) { cell in
                        Self.withCString(json) { json in zkloc_prove(dir, cell, json) }
                    }
                }
            }
        #else
            throw NativeError.notBuilt("ZKLoc.xcframework")
        #endif
    }

    func cell(latitude: Double, longitude: Double, resolution: Int) throws -> Region {
        #if canImport(ZKLocFFI)
            return try Self.call("zkloc_cell", Region.self) {
                zkloc_cell(latitude, longitude, numericCast(resolution))
            }
        #else
            throw NativeError.notBuilt("ZKLoc.xcframework")
        #endif
    }

    func outline(_ cell: String) throws -> Outline {
        #if canImport(ZKLocFFI)
            return try Self.call("zkloc_outline", Outline.self) {
                Self.withCString(cell) { zkloc_outline($0) }
            }
        #else
            throw NativeError.notBuilt("ZKLoc.xcframework")
        #endif
    }

    #if canImport(ZKLocFFI)
        /// Every zkloc function returns a JSON C string the caller frees, or {"error": ...}.
        private static func call<T: Decodable>(
            _ name: String, _ type: T.Type, _ body: () -> UnsafeMutablePointer<CChar>?
        ) throws -> T {
            guard let pointer = body() else { throw NativeError.failed(name, "returned nothing") }
            defer { zkloc_free(pointer) }
            let data = Data(String(cString: pointer).utf8)
            if let failure = try? JSONDecoder().decode(NativeFailure.self, from: data) {
                throw NativeError.failed(name, failure.error)
            }
            do {
                return try JSONDecoder().decode(type, from: data)
            } catch {
                throw NativeError.failed(name, "unexpected reply \(String(decoding: data, as: UTF8.self))")
            }
        }

        /// cgo exports take `char*`, not `const char*`, so hand over a mutable copy.
        private static func withCString<R>(_ text: String, _ body: (UnsafeMutablePointer<CChar>) -> R) -> R {
            let copy = strdup(text)!
            defer { free(copy) }
            return body(copy)
        }
    #endif
}

private struct NativeFailure: Decodable {
    let error: String
}

/// The native calls read `size.pixels` bytes of each plane, so the planes
/// must be exactly that long, and not empty.
private func checkPlanes(r: [UInt8], g: [UInt8], b: [UInt8], size: OriginalSize) throws {
    let pixels = size.pixels
    guard size.width > 0, size.height > 0, (1...OriginalSize.maxPixels).contains(pixels) else {
        throw CaptureError("an original must have 1 to \(OriginalSize.maxPixels) pixels, not \(size.width)×\(size.height)")
    }
    guard r.count == pixels, g.count == pixels, b.count == pixels else {
        throw CaptureError("the planes are not \(size.width)×\(size.height)")
    }
}

/// The crop prover of crates/crop-proof with its parameters loaded, from
/// ProvenanceRust.xcframework. The parameters stay in memory while this object
/// lives. The capture does not call it yet: the laptop still makes the crop
/// proof, and only the benchmark runs this.
final class CropProver: @unchecked Sendable {
    #if canImport(CropProofFFI)
        /// Rust only reads the parameters, so calls may overlap.
        private let handle: OpaquePointer
    #endif

    /// Loads `metadata.json` and `prover.bin` from `params`, a directory that
    /// `provenance setup` wrote.
    init(params: URL) throws {
        #if canImport(CropProofFFI)
            var loaded: OpaquePointer?
            let status = params.path(percentEncoded: false).withCString { crop_proof_prover_load($0, &loaded) }
            guard status == CROP_PROOF_OK, let loaded else { throw cropProofFailure("crop_proof_prover_load", status) }
            handle = loaded
        #else
            throw NativeError.notBuilt("ProvenanceRust.xcframework")
        #endif
    }

    deinit {
        #if canImport(CropProofFFI)
            crop_proof_prover_free(handle)
        #endif
    }

    /// A proof that `crop` of the original, turned by `turn`, is a crop of
    /// the image with the original's fingerprint. Blocks until the proof is
    /// made and uses every core, so call it off the main thread.
    func prove(r: [UInt8], g: [UInt8], b: [UInt8], size: OriginalSize, crop: CropRect, turn: Turn) throws -> Data {
        try checkPlanes(r: r, g: g, b: b, size: size)
        #if canImport(CropProofFFI)
            var proof = [UInt8](repeating: 0, count: Int(CROP_PROOF_CAPACITY))
            var length = 0
            let status = withPlanes(r, g, b) { r, g, b in
                proof.withUnsafeMutableBufferPointer { out in
                    crop_proof_prove(
                        handle, r, g, b, size.width, size.height, crop.native, UInt8(turn.quarters),
                        out.baseAddress!, out.count, &length)
                }
            }
            guard status == CROP_PROOF_OK else { throw cropProofFailure("crop_proof_prove", status) }
            return Data(proof.prefix(length))
        #else
            throw NativeError.notBuilt("ProvenanceRust.xcframework")
        #endif
    }
}

/// The crop verifier with its parameters loaded, for a check on the device
/// right after proving.
final class CropVerifier: @unchecked Sendable {
    #if canImport(CropProofFFI)
        private let handle: OpaquePointer
    #endif

    /// Loads `metadata.json` and `verifier.bin` from `params`.
    init(params: URL) throws {
        #if canImport(CropProofFFI)
            var loaded: OpaquePointer?
            let status = params.path(percentEncoded: false).withCString { crop_proof_verifier_load($0, &loaded) }
            guard status == CROP_PROOF_OK, let loaded else { throw cropProofFailure("crop_proof_verifier_load", status) }
            handle = loaded
        #else
            throw NativeError.notBuilt("ProvenanceRust.xcframework")
        #endif
    }

    deinit {
        #if canImport(CropProofFFI)
            crop_proof_verifier_free(handle)
        #endif
    }

    /// Throws unless a reader would accept `proof` for the published image
    /// and the fingerprint that this original gives.
    func verify(_ proof: Data, r: [UInt8], g: [UInt8], b: [UInt8], size: OriginalSize, crop: CropRect, turn: Turn) throws {
        try checkPlanes(r: r, g: g, b: b, size: size)
        guard !proof.isEmpty else { throw CaptureError("the crop proof is empty") }
        #if canImport(CropProofFFI)
            let status = withPlanes(r, g, b) { r, g, b in
                proof.withUnsafeBytes { proof in
                    crop_proof_verify(
                        handle, r, g, b, size.width, size.height, crop.native, UInt8(turn.quarters),
                        proof.bindMemory(to: UInt8.self).baseAddress!, proof.count)
                }
            }
            guard status == CROP_PROOF_OK else { throw cropProofFailure("crop_proof_verify", status) }
        #else
            throw NativeError.notBuilt("ProvenanceRust.xcframework")
        #endif
    }
}

#if canImport(CropProofFFI)
    /// The error for a crop proof call that returned `status`. Rust keeps the
    /// reason per thread, so this must run on the thread of the failed call.
    private func cropProofFailure(_ call: String, _ status: Int32) -> NativeError {
        let reason = String(cString: crop_proof_last_error())
        return .failed(call, reason.isEmpty ? "status \(status)" : reason)
    }

    /// The base addresses of three planes that `checkPlanes` accepted.
    private func withPlanes<R>(
        _ r: [UInt8], _ g: [UInt8], _ b: [UInt8],
        _ body: (UnsafePointer<UInt8>, UnsafePointer<UInt8>, UnsafePointer<UInt8>) -> R
    ) -> R {
        r.withUnsafeBufferPointer { r in
            g.withUnsafeBufferPointer { g in
                b.withUnsafeBufferPointer { b in body(r.baseAddress!, g.baseAddress!, b.baseAddress!) }
            }
        }
    }

    extension CropRect {
        /// Rust refuses a rectangle that does not fit the original. It reads
        /// a negative number as a huge one, which does not fit either.
        fileprivate var native: crop_proof_rect {
            crop_proof_rect(x: x, y: y, width: width, height: height)
        }
    }
#endif
