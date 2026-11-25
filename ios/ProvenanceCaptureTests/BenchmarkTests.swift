import Darwin
import UIKit
import XCTest

@testable import ProvenanceCapture

/// What a capture costs on this device, for crates/bench, which reads the
/// `BENCH` lines. Opt in with `TEST_RUNNER_BENCH_SAMPLES=<n>`. The harness
/// copies the location parameters into Fixtures/bench, so the test bundle
/// holds them.
@MainActor
final class BenchmarkTests: XCTestCase {
    private let latitude = -34.5478
    private let longitude = -58.4462
    private let saltOne = "0x" + String(repeating: "0", count: 63) + "1"
    private let cell = "87c2e3020ffffff"
    private let envelope = "0x0c5fe416c2846000ee1e623e19158db8cb35dbb69617a5c99ea523839e75ed29"
    /// The original of a 4:3 photo.
    private let size = OriginalSize(width: 836, height: 627)
    /// A photo is an occasional event. Back to back, the rounds would measure
    /// a hot phone instead of a capture.
    private let pause: TimeInterval = 15

    func testMeasureACapture() throws {
        guard let samples = ProcessInfo.processInfo.environment["BENCH_SAMPLES"].flatMap(Int.init) else {
            throw XCTSkip("set TEST_RUNNER_BENCH_SAMPLES to the number of samples")
        }
        // A locked phone suspends the host app and stretches every timing.
        UIApplication.shared.isIdleTimerDisabled = true
        defer { UIApplication.shared.isIdleTimerDisabled = false }

        let params = try XCTUnwrap(Bundle(for: Self.self).resourceURL)
        let planes = try Native.testImage(size)
        // Round 0 is the warmup, as on the laptop.
        for round in 0...samples {
            Thread.sleep(forTimeInterval: pause)
            try measure("fingerprint", round) {
                _ = try Native.library.fingerprint(r: planes.r, g: planes.g, b: planes.b, size: size)
                return 0
            }
            let hints = try Native.library.hints(latitude: latitude, longitude: longitude)
            let secrets = LocationSecrets(latitude: latitude, longitude: longitude, salt: saltOne, hints: hints)
            try measure("envelope", round) {
                XCTAssertEqual(try Native.library.envelope(secrets), envelope)
                return 0
            }
            try measure("location-prove", round) {
                let proof = try Native.library.prove(params: params, cell: cell, secrets: secrets)
                XCTAssertEqual(proof.envelope, envelope)
                return try XCTUnwrap(Data(base64Encoded: proof.proof)).count
            }
        }
    }

    /// `work` returns the size of what it made.
    private func measure(_ operation: String, _ round: Int, _ work: () throws -> Int) throws {
        let started = ContinuousClock.now
        let bytes = try work()
        let elapsed = (ContinuousClock.now - started) / .seconds(1)
        let thermal = ProcessInfo.processInfo.thermalState.rawValue
        print("BENCH \(operation) \(round) \(elapsed) \(bytes) \(peakFootprint()) \(thermal)")
    }

    /// The most memory iOS has charged this process so far, the figure it
    /// compares with the app's limit.
    private func peakFootprint() -> UInt64 {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let status = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return status == KERN_SUCCESS ? UInt64(info.ledger_phys_footprint_peak) : 0
    }
}
