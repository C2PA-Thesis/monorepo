import CryptoKit
import Darwin
import UIKit
import XCTest
import os

@testable import ProvenanceCapture

/// What a capture and a crop proof cost on this device, for crates/bench,
/// which reads the `BENCH` lines. Opt in with `TEST_RUNNER_BENCH_SAMPLES=<n>`.
/// The harness copies the parameters into Fixtures/bench, so the test bundle
/// holds them.
///
/// The peak footprint is the largest the process ever had, so each test here
/// must run alone, in its own `xcodebuild test -only-testing:` call. In one
/// process, the heaviest test would set the figure of every test after it.
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
    /// A crop proof keeps every core busy for much longer than a capture, so
    /// the phone gets longer to cool down.
    private let cropPause: TimeInterval = 30

    func testMeasureACapture() throws {
        let samples = try sampleCount()
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

    /// The crop proof at the reference case of crates/bench: a 1024x512
    /// original, its left 512x512, no turn. Each round loads the parameters
    /// and proves, as one laptop sample does, then checks the proof.
    func testMeasureACropProof() async throws {
        let samples = try sampleCount()
        let params = try XCTUnwrap(Bundle(for: Self.self).resourceURL)
        guard FileManager.default.fileExists(atPath: params.appending(path: "prover.bin").path(percentEncoded: false)) else {
            throw XCTSkip("copy the crop parameters into ProvenanceCaptureTests/Fixtures/bench/crop")
        }
        UIApplication.shared.isIdleTimerDisabled = true
        defer { UIApplication.shared.isIdleTimerDisabled = false }

        let job = CropJob(params: params, planes: try Native.testImage(CropJob.size))
        for round in 0...samples {
            try await Task.sleep(for: .seconds(cropPause))
            // Off the main thread: the app that hosts the tests must stay
            // responsive for the whole proof.
            try await offMain { try job.measure(round) }
        }
    }

    /// How much memory iOS lets this app have. Opt in with
    /// `TEST_RUNNER_BENCH_MEMORY_LIMIT=1`, on a device. It takes memory in
    /// steps and prints the footprint and the headroom after each. It stops
    /// with about 200 MB of headroom left, so iOS does not kill the app. The
    /// last line is the limit: the footprint plus the headroom.
    func testMeasureTheMemoryLimit() throws {
        guard ProcessInfo.processInfo.environment["BENCH_MEMORY_LIMIT"] == "1" else {
            throw XCTSkip("set TEST_RUNNER_BENCH_MEMORY_LIMIT=1 to measure the app's memory limit")
        }
        // The Simulator has no limit and reports no headroom. Without one,
        // the loop below would take all the memory of the Mac.
        guard os_proc_available_memory() > 0 else {
            throw XCTSkip("this process has no memory limit to measure")
        }
        let reserve = 200 << 20
        let coarse = 256 << 20
        let fine = 16 << 20
        let physical = ProcessInfo.processInfo.physicalMemory

        var blocks: [UnsafeMutableRawBufferPointer] = []
        defer { blocks.forEach { $0.deallocate() } }
        var taken = 0
        while taken < physical {
            let available = os_proc_available_memory()
            // Small steps near the end, so the last one cannot pass the limit.
            let step = available >= reserve + 2 * coarse ? coarse : fine
            guard available >= reserve + step else { break }
            let block = UnsafeMutableRawBufferPointer.allocate(byteCount: step, alignment: 16384)
            // Random bytes: every page is written, and none can be shared or
            // stored smaller.
            arc4random_buf(block.baseAddress, step)
            blocks.append(block)
            taken += step
            log("LIMIT step \(blocks.count) taken \(taken) footprint \(taskMemory().footprint) available \(os_proc_available_memory())")
        }
        let footprint = taskMemory().footprint
        let available = UInt64(os_proc_available_memory())
        log("LIMIT physical \(physical) footprint \(footprint) available \(available)")
        log("LIMIT measured \(footprint + available)")
    }

    private func sampleCount() throws -> Int {
        guard let samples = ProcessInfo.processInfo.environment["BENCH_SAMPLES"].flatMap(Int.init) else {
            throw XCTSkip("set TEST_RUNNER_BENCH_SAMPLES to the number of samples")
        }
        return samples
    }

    /// `work` returns the size of what it made.
    private func measure(_ operation: String, _ round: Int, _ work: () throws -> Int) throws {
        let started = ContinuousClock.now
        let bytes = try work()
        let elapsed = (ContinuousClock.now - started) / .seconds(1)
        let thermal = ProcessInfo.processInfo.thermalState.rawValue
        print("BENCH \(operation) \(round) \(elapsed) \(bytes) \(taskMemory().peak) \(thermal)")
    }
}

/// One crop proof at the reference case, measured and checked.
private struct CropJob: Sendable {
    static let size = OriginalSize(width: 1024, height: 512)
    static let crop = CropRect(x: 0, y: 0, width: 512, height: 512)
    static let sampleInterval: TimeInterval = 0.5

    let params: URL
    let planes: (r: [UInt8], g: [UInt8], b: [UInt8])

    /// Prints `CROP-MEMORY round moment footprint headroom` before, during
    /// and after the proof and once the prover is freed, `CROP-PROOF` with
    /// the proof's digest, the time of each step, then the `BENCH` line. The `BENCH` line comes last,
    /// so a proof that does not verify leaves no measurement.
    func measure(_ round: Int) throws {
        logMemory(round, "before")
        let made = try prove(round)
        logMemory(round, "freed")

        let started = ContinuousClock.now
        let verifier = try CropVerifier(params: params)
        try verifier.verify(made.proof, r: planes.r, g: planes.g, b: planes.b, size: Self.size, crop: Self.crop, turn: .zero)
        let verify = seconds(since: started)
        if round == 0 {
            // Once is enough to know that the check can fail.
            var damaged = made.proof
            damaged[damaged.count / 2] ^= 1
            let accepted = (try? verifier.verify(damaged, r: planes.r, g: planes.g, b: planes.b, size: Self.size, crop: Self.crop, turn: .zero)) != nil
            guard !accepted else { throw CaptureError("the check accepted a damaged proof") }
        }

        log("CROP-PHASES \(round) load \(made.load) prove \(made.prove) verify \(verify)")
        log("BENCH crop-prove \(round) \(made.load + made.prove) \(made.proof.count) \(made.peak) \(made.thermal)")
    }

    /// A proof and what it cost.
    private struct Made {
        let proof: Data
        let load: Double
        let prove: Double
        let peak: UInt64
        let thermal: Int
    }

    /// Loads the prover, proves, and frees the prover on return. The check
    /// that follows must not run on top of the prover's parameters: the peak
    /// is the largest footprint of the whole process, and a later round
    /// would report the check's memory once the proof needs less.
    private func prove(_ round: Int) throws -> Made {
        let sampler = sampleMemory(round)
        defer { sampler.cancel() }

        let started = ContinuousClock.now
        let prover = try CropProver(params: params)
        let load = seconds(since: started)
        let loaded = ContinuousClock.now
        let proof = try prover.prove(r: planes.r, g: planes.g, b: planes.b, size: Self.size, crop: Self.crop, turn: .zero)
        let prove = seconds(since: loaded)
        sampler.cancel()
        // Read here, before the check, which is not part of the proof.
        let peak = taskMemory().peak
        let thermal = ProcessInfo.processInfo.thermalState.rawValue
        logMemory(round, "after")
        // The proof is the same bytes for the same image and parameters, so
        // this digest must equal the one crates/crop-proof-ffi/c/check.sh
        // prints on the laptop. Another digest means another prover, most
        // likely a library that was not rebuilt. Printed before the check,
        // so a proof that fails it still names itself.
        let digest = SHA256.hash(data: proof).map { String(format: "%02x", $0) }.joined()
        log("CROP-PROOF \(round) \(proof.count) \(digest)")
        return Made(proof: proof, load: load, prove: prove, peak: peak, thermal: thermal)
    }

    private func seconds(since start: ContinuousClock.Instant) -> Double {
        (ContinuousClock.now - start) / .seconds(1)
    }

    /// Logs the memory every `sampleInterval` until cancelled.
    private func sampleMemory(_ round: Int) -> any DispatchSourceTimer {
        let started = ContinuousClock.now
        // The proof keeps every core busy; a low priority would starve the timer.
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .userInteractive))
        timer.schedule(deadline: .now() + Self.sampleInterval, repeating: Self.sampleInterval)
        timer.setEventHandler {
            logMemory(round, String(format: "%.1f", seconds(since: started)))
        }
        timer.resume()
        return timer
    }
}

/// The footprint is what iOS compares with the app's limit, and the headroom
/// (`os_proc_available_memory`) is how far the app is from it.
private func logMemory(_ round: Int, _ moment: String) {
    log("CROP-MEMORY \(round) \(moment) \(taskMemory().footprint) \(os_proc_available_memory())")
}

/// Prints a line now. If iOS kills the process for its memory, the lines
/// before the kill are the only record, so they must not wait in a buffer.
private func log(_ line: String) {
    print(line)
    fflush(nil)
}

/// The memory iOS charges this process now, and the most it ever charged.
private func taskMemory() -> (footprint: UInt64, peak: UInt64) {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let status = withUnsafeMutablePointer(to: &info) { pointer in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    guard status == KERN_SUCCESS else { return (0, 0) }
    return (info.phys_footprint, UInt64(info.ledger_phys_footprint_peak))
}

/// Runs blocking work on another thread and waits for it.
private func offMain<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { continuation in
        DispatchQueue.global(qos: .userInitiated).async {
            continuation.resume(with: Result(catching: work))
        }
    }
}
