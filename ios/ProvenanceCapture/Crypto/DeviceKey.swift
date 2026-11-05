import CryptoKit
import Foundation
import LocalAuthentication
import Security

enum DeviceKeyKind: String, Codable, Sendable {
    case secureEnclave = "secure-enclave"
    case software
}

/// The phone's signing key. The laptop trusts it after pairing and checks
/// every receipt against it.
protocol DeviceKey: Sendable {
    var kind: DeviceKeyKind { get }
    /// The public key as SubjectPublicKeyInfo DER.
    var spki: Data { get }
    /// DER ECDSA P-256 SHA-256 signature over `message`.
    func sign(_ message: Data) throws -> Data
}

extension DeviceKey {
    /// SHA-256 of the SPKI DER, as the receipt and the server name the key.
    var id: String { spki.sha256Hex }

    var pem: String {
        let base64 = spki.base64EncodedString()
        var lines: [Substring] = []
        var rest = Substring(base64)
        while !rest.isEmpty {
            lines.append(rest.prefix(64))
            rest = rest.dropFirst(64)
        }
        return "-----BEGIN PUBLIC KEY-----\n" + lines.joined(separator: "\n") + "\n-----END PUBLIC KEY-----\n"
    }
}

/// A key inside the Secure Enclave. Only its encrypted handle leaves the
/// enclave, and every signature needs Face ID, Touch ID or the passcode.
struct SecureEnclaveDeviceKey: DeviceKey {
    let handle: Data
    let spki: Data
    var kind: DeviceKeyKind { .secureEnclave }

    static func create() throws -> SecureEnclaveDeviceKey {
        guard SecureEnclave.isAvailable else { throw CaptureError("this device has no Secure Enclave") }
        var error: Unmanaged<CFError>?
        guard
            let access = SecAccessControlCreateWithFlags(
                nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .userPresence], &error)
        else { throw error!.takeRetainedValue() as any Error }
        let key = try SecureEnclave.P256.Signing.PrivateKey(accessControl: access)
        return SecureEnclaveDeviceKey(handle: key.dataRepresentation, spki: key.publicKey.derRepresentation)
    }

    init(handle: Data, spki: Data) {
        self.handle = handle
        self.spki = spki
    }

    init(handle: Data) throws {
        let key = try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: handle)
        self.init(handle: handle, spki: key.publicKey.derRepresentation)
    }

    func sign(_ message: Data) throws -> Data {
        let context = LAContext()
        context.localizedReason = "Sign this capture"
        let key = try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: handle, authenticationContext: context)
        return try key.signature(for: message).derRepresentation
    }
}

/// The Simulator has no Secure Enclave: a software key kept in the Keychain stands in.
struct SoftwareDeviceKey: DeviceKey {
    let raw: Data
    var kind: DeviceKeyKind { .software }

    init(raw: Data) throws {
        _ = try P256.Signing.PrivateKey(rawRepresentation: raw)
        self.raw = raw
    }

    static func create() -> SoftwareDeviceKey {
        // A freshly generated key always round-trips.
        try! SoftwareDeviceKey(raw: P256.Signing.PrivateKey().rawRepresentation)
    }

    var spki: Data { (try? P256.Signing.PrivateKey(rawRepresentation: raw))?.publicKey.derRepresentation ?? Data() }

    func sign(_ message: Data) throws -> Data {
        try P256.Signing.PrivateKey(rawRepresentation: raw).signature(for: message).derRepresentation
    }
}

enum DeviceKeys {
    #if targetEnvironment(simulator)
        static let kind = DeviceKeyKind.software
    #else
        static let kind = DeviceKeyKind.secureEnclave
    #endif

    static func loadOrCreate() throws -> any DeviceKey {
        let account = kind.rawValue
        if let stored = try Keychain.read(account: account) {
            switch kind {
            case .secureEnclave: return try SecureEnclaveDeviceKey(handle: stored)
            case .software: return try SoftwareDeviceKey(raw: stored)
            }
        }
        let key: any DeviceKey
        let secret: Data
        switch kind {
        case .secureEnclave:
            let created = try SecureEnclaveDeviceKey.create()
            (key, secret) = (created, created.handle)
        case .software:
            let created = SoftwareDeviceKey.create()
            (key, secret) = (created, created.raw)
        }
        try Keychain.write(secret, account: account)
        return key
    }
}

/// Generic-password items, readable only while unlocked and never synced or backed up to another device.
enum Keychain {
    static let service = "io.github.c2pa-thesis.capture.device-key"

    static func read(account: String) throws -> Data? {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne,
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else { throw keychainError(status) }
        return data
    }

    static func write(_ data: Data, account: String) throws {
        let item: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecAttrAccessible: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            kSecValueData: data,
        ]
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else { throw keychainError(status) }
    }

    private static func keychainError(_ status: OSStatus) -> CaptureError {
        let text = SecCopyErrorMessageString(status, nil) as String? ?? "status \(status)"
        return CaptureError("the Keychain refused the device key: \(text)")
    }
}
