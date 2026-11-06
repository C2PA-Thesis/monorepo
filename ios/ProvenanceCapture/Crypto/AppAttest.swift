import CryptoKit
import DeviceCheck
import Foundation

/// App Attest for pairing. Apple certifies a key of its own in the Secure
/// Enclave, over a hash that names the pairing token and the receipt key, so
/// the laptop learns that this app, signed by our team, asked to pair that key.
enum PairingAttestation {
    /// The same bytes as `pairing_client_data` in crates/provenance/src/serve.rs.
    static func clientData(token: String, device: String) -> Data {
        Data("io.github.c2pa-thesis.pair.v2\n\(token)\n\(device)".utf8)
    }

    static func clientDataHash(token: String, device: String) -> Data {
        Data(SHA256.hash(data: clientData(token: token, device: device)))
    }

    enum Outcome: Sendable {
        case attested(AppAttestation)
        /// Pair without one; the laptop then shows the key as declared.
        case unavailable(String)
    }

    /// A fresh App Attest key per pairing: the app makes no assertions later,
    /// so no key id needs to be kept.
    static func attest(token: String, device: String) async -> Outcome {
        await AppAttest.attest(clientDataHash: clientDataHash(token: token, device: device))
    }
}

/// App Attest for registering with a known server, over the server's
/// challenge and the receipt key.
enum RegistrationAttestation {
    /// The same bytes as `registration_client_data` in crates/provenance/src/register.rs.
    static func clientData(challenge: String, device: String) -> Data {
        Data("io.github.c2pa-thesis.register.v1\n\(challenge)\n\(device)".utf8)
    }

    static func clientDataHash(challenge: String, device: String) -> Data {
        Data(SHA256.hash(data: clientData(challenge: challenge, device: device)))
    }

    /// One key per registration. A phone registers once per server and
    /// install, which keeps Apple's count of keys on the device low.
    static func attest(challenge: String, device: String) async -> PairingAttestation.Outcome {
        await AppAttest.attest(clientDataHash: clientDataHash(challenge: challenge, device: device))
    }
}

enum AppAttest {
    static func attest(clientDataHash hash: Data) async -> PairingAttestation.Outcome {
        #if targetEnvironment(simulator)
            return .unavailable("the Simulator has no App Attest")
        #else
            let service = DCAppAttestService.shared
            guard service.isSupported else { return .unavailable("this device does not support App Attest") }
            do {
                let keyId = try await service.generateKey()
                let object: Data
                do {
                    object = try await service.attestKey(keyId, clientDataHash: hash)
                } catch let error as DCError where error.code == .serverUnavailable {
                    // Apple asks to retry with the same key and hash.
                    try await Task.sleep(for: .seconds(2))
                    object = try await service.attestKey(keyId, clientDataHash: hash)
                }
                return .attested(AppAttestation(keyId: keyId, object: object.base64EncodedString()))
            } catch {
                return .unavailable("App Attest failed: \(error.localizedDescription)")
            }
        #endif
    }
}
