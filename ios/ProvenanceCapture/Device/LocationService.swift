import CoreLocation
import Foundation
import Observation

/// A position as the capture records it.
struct Fix: Equatable, Sendable {
    let latitude: Double
    let longitude: Double
    let accuracy: Double
    let timestamp: Date
    /// iOS says the fix came from software, such as a GPX file in Xcode.
    let simulated: Bool
    /// iOS says the fix came from an external accessory, such as a CarPlay unit.
    let accessory: Bool

    init(_ location: CLLocation) {
        latitude = location.coordinate.latitude
        longitude = location.coordinate.longitude
        accuracy = location.horizontalAccuracy
        timestamp = location.timestamp
        accessory = location.sourceInformation?.isProducedByAccessory ?? false
        #if targetEnvironment(simulator)
            simulated = true
        #else
            simulated = location.sourceInformation?.isSimulatedBySoftware ?? false
        #endif
    }
}

@MainActor
@Observable
final class LocationService: NSObject, CLLocationManagerDelegate {
    enum Status: Equatable {
        case waiting(String)
        case ok(String)
        case error(String, help: String?)
    }

    /// A capture needs a fix taken at most this long before or after the shutter.
    static let maxAge: TimeInterval = 10
    static let purposeKey = "capture"

    private(set) var status = Status.waiting("Waiting for GPS")
    private(set) var last: Fix?
    @ObservationIgnored private let manager = CLLocationManager()
    @ObservationIgnored private var waiters: [UUID: Waiter] = [:]

    private struct Waiter {
        let shutter: Date
        let continuation: CheckedContinuation<Fix, any Error>
    }

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBest
        manager.distanceFilter = kCLDistanceFilterNone
    }

    func start() {
        switch manager.authorizationStatus {
        case .notDetermined:
            manager.requestWhenInUseAuthorization()
        case .denied, .restricted:
            denied()
        default:
            authorized()
        }
    }

    /// Asks again for precise location. The temporary grant ends with the app
    /// session, so this runs before every capture, not only at launch.
    func requestFullAccuracy() async {
        guard manager.accuracyAuthorization == .reducedAccuracy, isAuthorized else { return }
        try? await manager.requestTemporaryFullAccuracyAuthorization(withPurposeKey: Self.purposeKey)
    }

    /// The fix for a photo taken at `shutter`: one at most `maxAge` from it,
    /// waiting no later than `shutter + maxAge` for the next one.
    func fix(at shutter: Date) async throws -> Fix {
        await requestFullAccuracy()
        if let failure = unavailable { throw failure }
        if let last, Self.matches(last, shutter) { return last }
        let deadline = shutter.addingTimeInterval(Self.maxAge)
        guard deadline > .now else { throw Self.noFix }
        let id = UUID()
        let timeout = Task { [weak self] in
            try? await Task.sleep(for: .seconds(deadline.timeIntervalSinceNow))
            self?.resume(id, throwing: Self.noFix)
        }
        defer { timeout.cancel() }
        return try await withCheckedThrowingContinuation { continuation in
            waiters[id] = Waiter(shutter: shutter, continuation: continuation)
        }
    }

    /// The 10 s rule stays in `maxAge` and the README; the screen says only what to do.
    private static let noFix = CaptureError("No GPS fix. Try again.")

    private static func matches(_ fix: Fix, _ shutter: Date) -> Bool {
        abs(fix.timestamp.timeIntervalSince(shutter)) <= maxAge
    }

    private var isAuthorized: Bool {
        [.authorizedWhenInUse, .authorizedAlways].contains(manager.authorizationStatus)
    }

    /// Why no fix can come at all, so a capture fails at once instead of waiting.
    private var unavailable: CaptureError? {
        switch manager.authorizationStatus {
        case .denied, .restricted: CaptureError("Location access is blocked for this app.")
        case .notDetermined: CaptureError("Allow location access for this app, then take the photo again.")
        default:
            manager.accuracyAuthorization == .reducedAccuracy
                ? CaptureError("Location is set to approximate. Allow precise location for this app.") : nil
        }
    }

    private func resume(_ id: UUID, throwing error: any Error) {
        waiters.removeValue(forKey: id)?.continuation.resume(throwing: error)
    }

    private func failAll(_ error: any Error) {
        let waiting = waiters
        waiters = [:]
        for waiter in waiting.values { waiter.continuation.resume(throwing: error) }
    }

    private func authorized() {
        if manager.accuracyAuthorization == .reducedAccuracy {
            manager.requestTemporaryFullAccuracyAuthorization(withPurposeKey: Self.purposeKey)
        }
        manager.startUpdatingLocation()
    }

    private func denied() {
        status = .error(
            "Location blocked",
            help: "Allow While Using the App and Precise Location in Settings › Capture › Location.")
        failAll(CaptureError("Location access is blocked for this app."))
    }

    private func receive(_ location: CLLocation) {
        guard location.horizontalAccuracy >= 0 else { return }
        let fix = Fix(location)
        #if !targetEnvironment(simulator)
            if fix.simulated {
                status = .error("Position comes from software", help: nil)
                return
            }
        #endif
        guard manager.accuracyAuthorization == .fullAccuracy else {
            status = .error(
                "Location is approximate",
                help: "Turn on Precise Location in Settings › Capture › Location.")
            return
        }
        last = fix
        let source = fix.simulated ? " · simulated" : fix.accessory ? " · accessory" : ""
        status = .ok("±\(Int(fix.accuracy.rounded())) m\(source)")
        for (id, waiter) in waiters where Self.matches(fix, waiter.shutter) {
            waiters.removeValue(forKey: id)
            waiter.continuation.resume(returning: fix)
        }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ client: CLLocationManager) {
        MainActor.assumeIsolated {
            switch self.manager.authorizationStatus {
            case .denied, .restricted: denied()
            case .authorizedWhenInUse, .authorizedAlways: authorized()
            default: break
            }
        }
    }

    nonisolated func locationManager(_ client: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        MainActor.assumeIsolated {
            if let location = locations.last { receive(location) }
        }
    }

    nonisolated func locationManager(_ client: CLLocationManager, didFailWithError error: any Error) {
        MainActor.assumeIsolated {
            if (error as? CLError)?.code == .denied {
                denied()
            } else if (error as? CLError)?.code == .locationUnknown {
                // Core Location keeps trying after this one.
                if last == nil { status = .waiting("Waiting for GPS") }
            } else if last == nil {
                status = .error("No GPS fix yet", help: nil)
            }
        }
    }
}
