import CryptoKit
import Foundation

/// The Groth16 proving key and circuit the phone proves with, downloaded once
/// per pairing and checked against the hashes in the pairing payload.
enum ProofParams {
    /// What location-proof expects next to the params. The payload hashes pin
    /// the files to the circuit the laptop runs, so writing it here is safe.
    static let circuitVersion = "sealed-hints-v1"

    static var directory: URL {
        URL.applicationSupportDirectory.appending(path: "params/location", directoryHint: .isDirectory)
    }

    static func fetch(
        from api: LaptopAPI, expected: [String: String], progress: @MainActor @Sendable (String) -> Void
    ) async throws {
        let manager = FileManager.default
        try manager.createDirectory(at: directory, withIntermediateDirectories: true)
        for name in PairingPayload.paramNames {
            guard let want = expected[name] else { throw CaptureError("the pairing code has no hash for \(name)") }
            let target = directory.appending(path: name)
            if manager.fileExists(atPath: target.path()), try sha256(of: target) == want { continue }
            await progress("Downloading \(name)…")
            let file = try await api.param(name)
            await progress("Checking \(name)…")
            let got = try sha256(of: file)
            guard got == want else {
                try? manager.removeItem(at: file)
                throw CaptureError("\(name) has SHA-256 \(Verdict.short(got)), the laptop announced \(Verdict.short(want))")
            }
            try? manager.removeItem(at: target)
            try manager.moveItem(at: file, to: target)
        }
        try Data((circuitVersion + "\n").utf8).write(to: directory.appending(path: "version"))
        var excluded = URLResourceValues()
        excluded.isExcludedFromBackup = true
        var folder = directory
        try folder.setResourceValues(excluded)
    }

    static func sha256(of file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return Data(hasher.finalize()).hex
    }
}
