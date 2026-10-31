import Foundation
import Observation
import UIKit

enum StepState: Equatable {
    case locked, active, done
}

/// One thing the phone does on its own while signing, with its timing.
struct LocalStep: Identifiable, Equatable {
    enum State: Equatable { case running, done, failed }

    let id = UUID()
    let title: String
    let started: Date
    var state = State.running
    var seconds: Double?
    var detail: String?
}

struct Shot {
    let photo: Photo
    let capturedAt: String
    let fix: Fix
}

enum Outcome {
    case accepted(Claim, image: Data, file: URL)
    case rejected(title: String, reason: String)
}

/// The capture flow, step by step as web/src/main.ts runs it: pair once, take
/// a photo, frame and crop it, choose the cell, then fingerprint, seal, prove
/// and sign on the phone and follow the laptop while it publishes.
@MainActor
@Observable
final class CaptureFlow {
    #if targetEnvironment(simulator)
        static let isSimulator = true
    #else
        static let isSimulator = false
    #endif

    let location = LocationService()

    // 1. Pairing
    private(set) var key: (any DeviceKey)?
    private(set) var pairing: Pairing?
    private(set) var pairStatus = "Creating this phone's signing key…"
    private(set) var pairFailed = false
    private(set) var pairingBusy = false
    private(set) var paramsStatus: String?
    /// The parameter download is running.
    private(set) var paramsBusy = false
    var showScanner = false
    var pasted = ""

    // 2. Photo
    private(set) var shot: Shot?
    private(set) var photoSummary = ""
    var showCamera = false

    // 3. Frame and crop
    private(set) var original: OriginalImage?
    var framePosition = 0.5 {
        didSet { if framePosition != oldValue { render() } }
    }
    var crop = CropPreset.leftHalf

    // 4. Region
    var resolution = Verdict.defaultResolution {
        didSet { if resolution != oldValue { scheduleLookup() } }
    }
    private(set) var region: Region?
    private(set) var cellText = ""
    private(set) var cellFailed = false
    /// A lookup for the size the slider shows has not finished yet.
    private(set) var cellPending = false

    // 5. Sign and send
    private(set) var localSteps: [LocalStep] = []
    private(set) var sending = false
    private(set) var acceptedID: String?

    // 6. The laptop
    private(set) var laptop: CaptureStatus?
    private(set) var laptopSummary = ""
    private(set) var storedOnly = false
    private(set) var runningSince: [String: Date] = [:]
    private(set) var outcome: Outcome?

    @ObservationIgnored private var lookups = 0
    @ObservationIgnored private var lookupTask: Task<Void, Never>?
    @ObservationIgnored private var renders = 0
    @ObservationIgnored private var photos = 0
    @ObservationIgnored private var submitTask: Task<Void, Never>?
    @ObservationIgnored private var followTask: Task<Void, Never>?

    /// The one pinned session to the paired laptop, replaced only when the pairing is.
    private(set) var api: LaptopAPI?
    var isPaired: Bool { pairing?.paramsReady == true }

    // MARK: Step states

    var pairState: StepState { isPaired ? .done : .active }
    var photoState: StepState { !isPaired ? .locked : shot == nil ? .active : .done }
    var composeState: StepState { shot == nil ? .locked : (sending || acceptedID != nil) ? .done : .active }
    var sendState: StepState { shot == nil ? .locked : acceptedID != nil ? .done : .active }
    var laptopState: StepState { acceptedID == nil ? .locked : outcome == nil ? .active : .done }

    var pairSummary: String {
        guard let pairing else { return "" }
        return "Trusted by the laptop as device \(Verdict.short(pairing.device))"
    }

    // MARK: 1. Pairing

    func boot() async {
        guard key == nil else { return }
        location.start()
        do {
            key = try await Task.detached { try DeviceKeys.loadOrCreate() }.value
        } catch {
            fail("Could not create this phone's key: \(describe(error))")
            return
        }
        if let saved = Pairing.load(), saved.device == key?.id {
            use(saved)
            if !saved.paramsReady { await fetchParams() }
            return
        }
        waitForPairing()
        if Self.isSimulator { await fetchFromMac() }
    }

    /// The Simulator asks the Mac's own `provenance serve` for the payload.
    func fetchFromMac() async {
        pairStatus = "Asking provenance serve on this Mac for a pairing code…"
        pairFailed = false
        do {
            await pair(with: try await LocalPairing.fetch())
        } catch {
            fail("Could not reach provenance serve on 127.0.0.1:8791: \(describe(error)). Start it, or paste the pairing code.")
        }
    }

    func pairPasted() async {
        do {
            await pair(with: try PairingPayload.parse(pasted, requireAddress: !Self.isSimulator))
        } catch {
            fail(describe(error))
        }
    }

    func scanned(_ code: String) async {
        showScanner = false
        do {
            await pair(with: try PairingPayload.parse(code, requireAddress: !Self.isSimulator))
        } catch {
            fail(describe(error))
        }
    }

    func pair(with payload: PairingPayload) async {
        guard let key, !pairingBusy else { return }
        pairingBusy = true
        defer { pairingBusy = false }
        pairFailed = false
        pairStatus = "Pairing with the laptop…"
        do {
            let token = payload.token
            let signature = try await Task.detached { try key.sign(Data(token.utf8)) }.value
            let request = PairRequest(
                token: token, publicKey: key.pem, signature: signature.base64EncodedString(), kind: key.kind.rawValue,
                name: UIDevice.current.name)
            let (base, paired) = try await reach(candidates(payload), pin: payload.pin, request: request, device: key.id)
            guard paired.device == key.id else {
                throw CaptureError("the laptop registered key \(Verdict.short(paired.device)), not this phone's")
            }
            let pairing = Pairing(
                base: base, urls: candidates(payload), pin: payload.pin, device: paired.device, kind: key.kind,
                params: payload.params, pairedAt: .now, paramsReady: false)
            pairing.save()
            use(pairing)
            await fetchParams()
        } catch {
            fail("Could not pair: \(describe(error))")
        }
    }

    /// Forgets the pairing, also one whose parameters never downloaded, and keeps the key.
    func pairAgain() {
        guard !pairingBusy, !paramsBusy else { return }
        Pairing.clear()
        retake()
        use(nil)
        paramsStatus = nil
        waitForPairing()
    }

    private func use(_ pairing: Pairing?) {
        api?.invalidate()
        self.pairing = pairing
        api = pairing.map { LaptopAPI(bases: [$0.base] + $0.urls, pin: $0.pin) }
    }

    private func candidates(_ payload: PairingPayload) -> [URL] {
        var urls = payload.serverURLs
        // The Simulator shares the Mac's network stack, so localhost works even with no LAN address.
        let port = urls.first?.port ?? PairingPayload.tlsPort
        if Self.isSimulator, let local = URL(string: "https://localhost:\(port)") {
            urls.insert(local, at: 0)
        }
        return urls
    }

    /// Tries every announced address in turn, round after round, until one
    /// answers or the time is up. Each try is short, so a dead address (a VPN
    /// or bridge interface) costs seconds, and the rounds cover the time the
    /// local network prompt stays open. The token is single use, so an address
    /// is first asked a question that carries no token, and the token is sent
    /// again only when the request with it never connected. A request that
    /// connected and then timed out or dropped may have spent the token.
    private func reach(_ urls: [URL], pin: String, request: PairRequest, device: String) async throws -> (URL, Paired) {
        guard !urls.isEmpty else { throw CaptureError("the pairing code names no address") }
        let seconds = Self.isSimulator ? 10 : 90
        let deadline = Date.now.addingTimeInterval(TimeInterval(seconds))
        let apis = urls.map { LaptopAPI(bases: [$0], pin: pin, patience: .pairing) }
        defer { apis.forEach { $0.invalidate() } }
        var last: URLError?
        var denied = false
        rounds: while true {
            for (url, api) in zip(urls, apis) {
                let host = url.host() ?? ""
                do {
                    do {
                        _ = try await api.trusted(device)
                    } catch where !(error is URLError) {
                        // Any answer at all shows the address is the pinned laptop.
                    }
                    do {
                        return (url, try await api.pair(request))
                    } catch let error as URLError where !HTTP.neverConnected(error) && error.code != .cancelled {
                        // The laptop may have registered the key and only the answer was lost.
                        if (try? await api.trusted(device)) == true { return (url, Paired(device: device)) }
                        throw CaptureError(
                            "the laptop at \(host) stopped answering after the pairing request reached it (\(describe(error))). The QR code is single use and may be spent: show a new one on the laptop and scan it again")
                    }
                } catch let error as URLError where error.code == .cancelled {
                    throw CaptureError("the laptop at \(host) does not hold the pinned certificate")
                } catch let error as URLError where HTTP.isConnectionFailure(error) {
                    last = error
                    denied = denied || HTTP.localNetworkDenied(error)
                }
                if Date.now >= deadline { break rounds }
            }
            pairStatus = denied ? Self.localNetworkHelp : "Looking for the laptop on this network…"
            try await Task.sleep(for: .seconds(2))
        }
        if denied { throw CaptureError(Self.localNetworkHelp) }
        let reason = last.map { " (\(describe($0)))" } ?? ""
        throw CaptureError(
            "no address in the pairing code answered within \(seconds) seconds\(reason). Check that this phone is on the same network as the laptop and that provenance serve is running")
    }

    static let localNetworkHelp =
        "This app may not use the local network, so it cannot reach the laptop. Tap Allow when iOS asks, or turn on Settings › Privacy & Security › Local Network › Capture, then pair again."

    private func fetchParams() async {
        guard var pairing, let api, !paramsBusy else { return }
        paramsBusy = true
        defer { paramsBusy = false }
        pairFailed = false
        pairStatus = "Downloading the location proof parameters…"
        do {
            try await ProofParams.fetch(from: api, expected: pairing.params) { [weak self] status in
                self?.paramsStatus = status
            }
            pairing.paramsReady = true
            pairing.save()
            self.pairing = pairing
            paramsStatus = nil
        } catch {
            paramsStatus = nil
            fail("Paired, but the proof parameters did not download: \(describe(error)). Download them again, or pair again if the laptop was set up anew.")
        }
    }

    func retryParams() async {
        await fetchParams()
    }

    private func waitForPairing() {
        pairFailed = false
        let id = key.map { Verdict.short($0.id) } ?? ""
        pairStatus = Self.isSimulator
            ? "This Simulator's key is \(id). It pairs with provenance serve on this Mac, or paste the pairing code."
            : "This phone's key is \(id). Scan the QR code the laptop shows."
    }

    private func fail(_ message: String) {
        pairStatus = message
        pairFailed = true
    }

    // MARK: 2. Photo

    #if targetEnvironment(simulator)
        func useSample() async {
            guard let url = Bundle.main.url(forResource: "sample", withExtension: "jpg"),
                let data = try? Data(contentsOf: url)
            else {
                photoSummary = "The sample photo is missing from the app."
                return
            }
            await photoTaken(data, at: .now)
        }
    #endif

    func openCamera() {
        guard !sending else { return }
        // Asked before the shutter, so the answer does not eat into the fix window.
        Task { await location.requestFullAccuracy() }
        showCamera = true
    }

    /// `at` is when the shutter fired, or when the picked photo reached the app.
    func photoTaken(_ data: Data, at time: Date) async {
        showCamera = false
        photos += 1
        let mine = photos
        let capturedAt = Timestamp.rfc3339(time)
        photoSummary = location.last == nil ? "Waiting for a GPS fix…" : ""
        do {
            async let decoded = Task.detached { try Photo(data: data) }.value
            let fix = try await location.fix(at: time)
            let photo = try await decoded
            guard mine == photos else { return }
            retake()
            shot = Shot(photo: photo, capturedAt: capturedAt, fix: fix)
            framePosition = 0.5
            render()
            photoSummary = "\(Verdict.time(capturedAt)), GPS ±\(Int(fix.accuracy.rounded())) m. Tap to retake."
            scheduleLookup(immediately: true)
        } catch {
            guard mine == photos else { return }
            photoSummary = describe(error)
        }
    }

    /// Drops the current shot and everything still running for it.
    func retake() {
        submitTask?.cancel()
        submitTask = nil
        followTask?.cancel()
        followTask = nil
        lookupTask?.cancel()
        // Results still in flight for the old shot see a newer count and are dropped.
        lookups += 1
        renders += 1
        shot = nil
        original = nil
        region = nil
        cellText = ""
        cellPending = false
        localSteps = []
        sending = false
        acceptedID = nil
        laptop = nil
        laptopSummary = ""
        storedOnly = false
        runningSince = [:]
        outcome = nil
    }

    // MARK: 3. Frame

    private func render() {
        guard let photo = shot?.photo else { return }
        renders += 1
        let mine = renders
        let position = framePosition
        Task {
            let rendered = try? await Task.detached { try OriginalImage.render(photo, position: position) }.value
            if mine == renders, let rendered { original = rendered }
        }
    }

    // MARK: 4. Region

    /// Average sizes only. The outline of a cell comes from the laptop, so the
    /// app asks for it only once that cell is published (see `ResultMap`).
    /// Asking while the slider moves would tell the laptop every finer cell
    /// the user looked at.
    var resolutionText: String {
        Verdict.resolutionText(resolution)
    }

    /// The width of an average cell at the chosen resolution, in metres.
    var cellWidth: Double {
        Verdict.averageKm2[resolution].squareRoot() * 1000
    }

    /// The fix is coarser than the cell is wide, so the photo may be in a neighbouring cell.
    var accuracyWarning: String? {
        guard region != nil, let fix = shot?.fix else { return nil }
        guard fix.accuracy * 2 >= cellWidth else { return nil }
        return "The GPS fix is only accurate to ±\(Int(fix.accuracy.rounded())) m, which is coarse for a cell this small. The photo may have been taken in a neighbouring cell."
    }

    var regionSummary: String {
        guard let region else { return "" }
        return "Cell \(region.cell), about \(Verdict.area(Verdict.averageKm2[region.resolution]))"
    }

    private func scheduleLookup(immediately: Bool = false) {
        lookupTask?.cancel()
        cellPending = shot != nil
        lookupTask = Task {
            if !immediately { try? await Task.sleep(for: .milliseconds(250)) }
            guard !Task.isCancelled else { return }
            await lookupCell()
        }
    }

    private func lookupCell() async {
        guard let fix = shot?.fix else {
            cellPending = false
            return
        }
        lookups += 1
        let mine = lookups
        let resolution = resolution
        do {
            // Computed on the phone only: the exact coordinate never reaches the laptop.
            let found = try await Task.detached {
                try Native.library.cell(latitude: fix.latitude, longitude: fix.longitude, resolution: resolution)
            }.value
            guard mine == lookups else { return }
            region = found
            cellPending = false
            cellFailed = false
            cellText =
                "Cell \(found.cell). The published file names this cell, never the point inside it. Cell outlines come from the laptop, so the map draws this one only after it is published."
        } catch {
            guard mine == lookups else { return }
            if region?.resolution != resolution { region = nil }
            cellPending = false
            cellFailed = true
            cellText = "\(describe(error)). Try another size."
        }
    }

    // MARK: 5. Sign and send

    var published: [String] {
        [
            "The \(crop.text) crop",
            region.map { "Cell \($0.cell) (about \(Verdict.area(Verdict.averageKm2[$0.resolution])))" } ?? "The cell",
            shot.map { "Capture time, \(Verdict.time($0.capturedAt))" } ?? "The capture time",
            "This phone's key id, \(Verdict.short(key?.id ?? ""))",
            "A fingerprint of the whole frame (384 numbers)",
            "An envelope that seals the coordinate and its hints",
            "A proof that the sealed hints map to the cell",
        ]
    }

    /// The region must be the one for the size the slider shows, not one still being replaced.
    var canSubmit: Bool {
        shot != nil && !cellPending && region?.resolution == resolution && original != nil && !sending && acceptedID == nil
    }

    func submit() {
        guard canSubmit else { return }
        submitTask = Task { await send() }
    }

    private func send() async {
        guard let shot, let region, let original, let key, let api, canSubmit else { return }
        sending = true
        localSteps = []
        let fix = shot.fix
        let capturedAt = shot.capturedAt
        let crop = crop
        let native = Native.library
        do {
            let fingerprint = try await local("Fingerprint the whole frame, on this phone") {
                let planes = original.channels()
                return try native.fingerprint(r: planes.r, g: planes.g, b: planes.b)
            }
            let secrets = try await local("Seal the coordinate and its hints in an envelope (MiMC)", detail: { Verdict.short($0.1) }) {
                let hints = try native.hints(latitude: fix.latitude, longitude: fix.longitude)
                let secrets = LocationSecrets(
                    latitude: fix.latitude, longitude: fix.longitude, salt: LocationSecrets.freshSalt(), hints: hints)
                return (secrets, try native.envelope(secrets))
            }
            let (sealed, envelope) = secrets
            let proof = try await local("Prove the sealed hints map to cell \(region.cell) (Groth16)") {
                try native.prove(params: ProofParams.directory, cell: region.cell, secrets: sealed)
            }
            guard proof.envelope == envelope else {
                throw CaptureError("the proof is for envelope \(Verdict.short(proof.envelope)), not \(Verdict.short(envelope))")
            }
            let receipt = try await local("Sign fingerprint and envelope with this phone's key", detail: { "device \(Verdict.short($0.device))" }) {
                try Receipt.sign(fingerprint: fingerprint, envelope: envelope, capturedAt: capturedAt, key: key)
            }
            let accepted = try await local("Send to the laptop, which checks it as a reader would", detail: { $0.id }) {
                let upload = CaptureUpload(
                    originalPng: try original.png().base64EncodedString(), receipt: receipt, cell: region.cell,
                    resolution: region.resolution, crop: crop, accuracyMeters: fix.accuracy, locationProof: proof.proof,
                    location: .init(simulated: fix.simulated, accessory: fix.accessory))
                return try await api.upload(upload)
            }
            try Task.checkCancellation()
            acceptedID = accepted.id
            follow(accepted.id, api: api)
        } catch {
            // A retake already reset the steps for the next shot.
            if Task.isCancelled { return }
            if let index = localSteps.indices.last, localSteps[index].state == .running {
                finish(index, .failed, detail: describe(error))
            } else {
                localSteps.append(LocalStep(title: "Prepare the capture", started: .now, state: .failed, seconds: 0, detail: describe(error)))
            }
            sending = false
        }
    }

    /// Runs `work` off the main actor as one timed row of the list. Cancelling
    /// the caller cancels `work`, so a retake stops a pending upload.
    private func local<T: Sendable>(
        _ title: String, detail: (T) -> String? = { _ in nil }, _ work: @escaping @Sendable () async throws -> T
    ) async throws -> T {
        try Task.checkCancellation()
        localSteps.append(LocalStep(title: title, started: .now))
        let index = localSteps.count - 1
        let task = Task.detached(priority: .userInitiated, operation: work)
        let value = try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
        try Task.checkCancellation()
        finish(index, .done, detail: detail(value))
        return value
    }

    private func finish(_ index: Int, _ state: LocalStep.State, detail: String?) {
        guard localSteps.indices.contains(index) else { return }
        localSteps[index].state = state
        localSteps[index].seconds = Date.now.timeIntervalSince(localSteps[index].started)
        localSteps[index].detail = detail
    }

    var sendSummary: String { acceptedID.map { "Signed and accepted as \($0)" } ?? "" }

    // MARK: 6. The laptop

    var timeline: [Timeline.Group] {
        let halted = laptop?.phase == .failed ? (laptop?.error ?? "failed") : nil
        return Timeline.groups(laptop?.events ?? [], halted: halted)
    }

    /// Polls until the laptop finishes. Connection failures are retried for a
    /// while; anything else, a laptop gone for too long, or a capture still
    /// running after `followLimit`, ends the follow with a message.
    private func follow(_ id: String, api: LaptopAPI) {
        followTask = Task {
            var status: CaptureStatus?
            var failingSince: Date?
            let giveUp = Date.now.addingTimeInterval(Self.followLimit)
            while !Task.isCancelled {
                guard Date.now < giveUp else {
                    laptopSummary = "Stopped following the capture"
                    outcome = .rejected(
                        title: "The laptop has not finished",
                        reason: "The capture \(id) is still running after \(Int(Self.followLimit / 60)) minutes; check the laptop.")
                    return
                }
                do {
                    status = try await api.status(id)
                    failingSince = nil
                } catch {
                    if Task.isCancelled { return }
                    let since = failingSince ?? .now
                    failingSince = since
                    let transient = (error as? URLError).map(HTTP.isConnectionFailure) ?? false
                    if !transient || Date.now.timeIntervalSince(since) > Self.laptopPatience {
                        laptopSummary = "Lost track of the capture"
                        outcome = .rejected(
                            title: transient ? "The laptop stopped answering" : "The laptop's status could not be read",
                            reason: "\(describe(error)). The capture \(id) may still finish there; check the laptop.")
                        return
                    }
                    laptopSummary = "The laptop is not answering, retrying…"
                    try? await Task.sleep(for: .milliseconds(1500))
                    continue
                }
                guard let current = status, !Task.isCancelled else { return }
                if current.phase == .stored {
                    storedOnly = true
                    laptopSummary = ""
                    return
                }
                laptop = current
                noteRunning()
                if !current.phase.isRunning { break }
                laptopSummary = current.phase == .queued ? "Waiting for another capture to finish" : "Working…"
                try? await Task.sleep(for: .milliseconds(700))
            }
            guard let status, !Task.isCancelled else { return }
            laptopSummary = switch status.phase {
            case .accepted: "Published and verified"
            case .rejected: "Rejected"
            default: "Failed"
            }
            await showResult(status, api: api)
        }
    }

    private func noteRunning() {
        for group in timeline {
            for item in group.items {
                if case .running = item.state, runningSince[item.id] == nil { runningSince[item.id] = .now }
            }
        }
    }

    /// How long a laptop may stay unreachable while the app follows a capture.
    static let laptopPatience: TimeInterval = 60
    /// How long the app follows one capture before it stops polling.
    static let followLimit: TimeInterval = 15 * 60

    private func showResult(_ status: CaptureStatus, api: LaptopAPI) async {
        guard status.phase == .accepted, let claim = status.claim else {
            outcome = .rejected(
                title: status.phase == .rejected ? "Rejected" : "Publishing failed", reason: status.error ?? "unknown error")
            return
        }
        do {
            let png = try await api.signedPNG(status.id)
            let file = URL.temporaryDirectory.appending(path: "\(status.id).png")
            try png.write(to: file)
            guard !Task.isCancelled else { return }
            outcome = .accepted(claim, image: png, file: file)
        } catch {
            guard !Task.isCancelled else { return }
            outcome = .rejected(title: "Verified, but the signed file did not download", reason: describe(error))
        }
    }
}
