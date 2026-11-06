import AVFoundation
import CoreGraphics
import CoreLocation
import Foundation
import Observation
import UIKit

/// One row of a progress list.
enum RowState: Equatable {
    case idle, running, done, failed
}

/// What the phone does right after the shutter, in order.
enum SealStep: Int, CaseIterable {
    case locate, fingerprint, seal, sign
}

/// What the phone does when the person publishes, before the laptop takes over.
enum SendStep: Int, CaseIterable {
    case prove, upload
}

struct Shot {
    let photo: Photo
    let capturedAt: String
    let fix: Fix
}

/// Everything fixed at the shutter: the original, the sealed
/// position and the device's signature over the fingerprint and the envelope.
struct Sealed: Sendable {
    let original: OriginalImage
    let secrets: LocationSecrets
    let envelope: String
    let receipt: Receipt
}

/// A failure in words a person can act on. `detail` keeps the raw reason for
/// the technical detail.
struct Failure: Equatable {
    enum Action: Equatable { case retake, retry, settings }

    let title: String
    let message: String
    var detail = ""
    var action = Action.retake
}

enum Outcome {
    case accepted(Claim, image: Data, file: URL)
    case rejected(Failure)
}

/// The screen the app shows.
enum Stage: Equatable {
    case launch, welcome, location, cameraAccess, pair, camera, sealing, photo, area, publishing, result
}

/// The capture flow: pair once, then camera first. The shutter seals the
/// photo on the phone (fingerprint, sealed position, signature); cropping and
/// choosing the area are optional; publishing proves the area on the phone
/// and follows the laptop while it proves the crop and verifies.
@MainActor
@Observable
final class CaptureFlow {
    #if targetEnvironment(simulator)
        static let isSimulator = true
    #else
        static let isSimulator = false
    #endif

    let location = LocationService()

    // Pairing
    private(set) var key: (any DeviceKey)?
    private(set) var pairing: Pairing?
    private(set) var pairStatus = "Creating key…"
    private(set) var pairFailed = false
    private(set) var pairingBusy = false
    private(set) var paramsStatus: String?
    /// The parameter download is running.
    private(set) var paramsBusy = false
    var showScanner = false
    var pasted = ""

    // Welcome
    private(set) var welcomed = UserDefaults.standard.bool(forKey: CaptureFlow.welcomedKey)
    private static let welcomedKey = "welcomed.v1"

    // Capture and seal
    private(set) var shot: Shot?
    /// The original, shown as soon as it renders.
    private(set) var preview: CGImage?
    /// The original's size, the space the crop is in. Set with the preview.
    private(set) var originalSize = CaptureFlow.noOriginal
    /// Stands in for the size until a photo renders; nothing shows it.
    private static let noOriginal = OriginalSize(width: 1, height: 1)
    private(set) var sealed: Sealed?
    private(set) var sealSteps: [SealStep: RowState] = [:]
    private(set) var sealFailure: Failure?
    /// Between the shutter and the end of the seal.
    private(set) var capturing = false

    // Review
    var crop = CropRect.whole(CaptureFlow.noOriginal)
    var turn = Turn.zero
    /// The photo step is done and the area step shows.
    var photoChosen = false
    var resolution = Verdict.defaultResolution {
        didSet { if resolution != oldValue { scheduleLookup() } }
    }
    private(set) var region: Region?
    /// The chosen cell's outline, computed on the phone.
    private(set) var area: Outline?
    private(set) var cellFailed = false
    /// A lookup for the size on screen has not finished yet.
    private(set) var cellPending = false

    // Publish
    private(set) var sendSteps: [SendStep: RowState] = [:]
    private(set) var sendFailure: Failure?
    private(set) var sending = false
    private(set) var acceptedID: String?

    // The laptop
    private(set) var laptop: CaptureStatus?
    /// "Queued" or "Reconnecting…" while following.
    private(set) var laptopNotice: String?
    private(set) var storedOnly = false
    private(set) var runningSince: [String: Date] = [:]
    private(set) var outcome: Outcome?
    let gallery = Gallery()

    @ObservationIgnored private var lookups = 0
    @ObservationIgnored private var lookupTask: Task<Void, Never>?
    @ObservationIgnored private var photos = 0
    @ObservationIgnored private var sealTask: Task<Void, Never>?
    @ObservationIgnored private var sendTask: Task<Void, Never>?
    @ObservationIgnored private var followTask: Task<Void, Never>?

    /// The one pinned session to the paired laptop, replaced only when the pairing is.
    private(set) var api: LaptopAPI?
    var isPaired: Bool { pairing?.paramsReady == true }

    /// The key and any saved pairing are loaded, so the stage below is the real one.
    private(set) var booted = false

    var stage: Stage {
        if !booted { return .launch }
        if outcome != nil || storedOnly { return .result }
        if sending || acceptedID != nil || sendFailure != nil { return .publishing }
        if capturing || sealFailure != nil { return .sealing }
        if sealed != nil { return photoChosen ? .area : .photo }
        if !isPaired, !welcomed, pairing == nil { return .welcome }
        if location.access == .notAsked || location.access == .denied { return .location }
        if needsCameraAccess { return .cameraAccess }
        if !isPaired { return .pair }
        return .camera
    }

    /// Something runs that the pair screen waits for.
    var pairWorking: Bool { !pairFailed && (key == nil || pairingBusy || paramsBusy) }

    /// The failure is a permission only Settings can give back.
    var pairNeedsSettings: Bool { pairFailed && pairStatus == Self.localNetworkHelp }

    func finishWelcome() {
        welcomed = true
        UserDefaults.standard.set(true, forKey: Self.welcomedKey)
    }

    func allowLocation() {
        location.start()
    }

    /// Set once the camera question was asked, so the stage moves on whatever the answer.
    private(set) var cameraAsked = false

    /// The Simulator has no camera, so it skips the question.
    var needsCameraAccess: Bool {
        !Self.isSimulator && !cameraAsked && AVCaptureDevice.authorizationStatus(for: .video) == .notDetermined
    }

    func allowCamera() async {
        _ = await AVCaptureDevice.requestAccess(for: .video)
        cameraAsked = true
    }

    // MARK: 1. Pairing

    func boot() async {
        guard key == nil else { return }
        location.resume()
        do {
            key = try await Task.detached { try DeviceKeys.loadOrCreate() }.value
        } catch {
            booted = true
            fail("Could not create this phone's key: \(describe(error))")
            return
        }
        if let saved = Pairing.load(), saved.device == key?.id {
            use(saved)
            booted = true
            if !saved.paramsReady { await fetchParams() }
            return
        }
        booted = true
        waitForPairing()
    }

    /// The Simulator asks the Mac's own `provenance serve` for the payload.
    func fetchFromMac() async {
        pairStatus = "Connecting…"
        pairFailed = false
        pairingBusy = true
        let payload: PairingPayload
        do {
            payload = try await LocalPairing.fetch()
        } catch {
            pairingBusy = false
            fail("provenance serve on this Mac did not answer: \(describe(error)).")
            return
        }
        pairingBusy = false
        await pair(with: payload)
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
        pairStatus = "Pairing…"
        do {
            let token = payload.token
            let signature = try await Task.detached { try key.sign(Data(token.utf8)) }.value
            var request = PairRequest(
                token: token, publicKey: key.pem, signature: signature.base64EncodedString(), kind: key.kind.rawValue,
                name: UIDevice.current.name)
            var attestNote: String?
            if key.kind == .secureEnclave {
                pairStatus = "Asking Apple to attest this app…"
                switch await PairingAttestation.attest(token: token, device: key.id) {
                case .attested(let attestation): request.attestation = attestation
                case .unavailable(let reason): attestNote = reason
                }
                pairStatus = "Pairing…"
            }
            let (base, paired) = try await reach(candidates(payload), pin: payload.pin, request: request, device: key.id)
            guard paired.device == key.id else {
                throw CaptureError("the laptop registered key \(Verdict.short(paired.device)), not this phone's")
            }
            let pairing = Pairing(
                base: base, urls: candidates(payload), pin: payload.pin, device: paired.device, kind: key.kind,
                shownKind: paired.kind, attestNote: paired.note ?? attestNote,
                params: payload.params, pairedAt: .now, paramsReady: false)
            pairing.save()
            use(pairing)
            await fetchParams()
        } catch {
            fail("Could not pair: \(describe(error))")
        }
    }

    /// Registers with a server compiled into the app, without a QR code. The
    /// pin comes from the build, the parameter digests from the server over
    /// the pinned connection, and trust from App Attest.
    func register(with server: KnownServer) async {
        guard let key, !pairingBusy else { return }
        pairingBusy = true
        defer { pairingBusy = false }
        pairFailed = false
        pairStatus = "Connecting to \(server.name)…"
        let api = LaptopAPI(bases: server.serverURLs, pin: server.pin)
        defer { api.invalidate() }
        do {
            guard key.kind == .secureEnclave else {
                throw CaptureError("registering needs the Secure Enclave and App Attest; scan a laptop's code instead")
            }
            let info = try await api.server()
            try info.validate()
            let challenge = try await api.challenge().challenge
            let signature = try await Task.detached { try key.sign(Data(challenge.utf8)) }.value
            pairStatus = "Asking Apple to attest this app…"
            let attestation: AppAttestation
            switch await RegistrationAttestation.attest(challenge: challenge, device: key.id) {
            case .attested(let attested): attestation = attested
            case .unavailable(let reason): throw CaptureError("\(server.name) needs App Attest, and \(reason)")
            }
            pairStatus = "Registering…"
            let paired = try await api.register(
                RegisterRequest(
                    challenge: challenge, publicKey: key.pem, signature: signature.base64EncodedString(),
                    kind: key.kind.rawValue, name: UIDevice.current.name, attestation: attestation))
            guard paired.device == key.id else {
                throw CaptureError("\(server.name) registered key \(Verdict.short(paired.device)), not this phone's")
            }
            var urls = server.serverURLs
            for url in info.serverURLs where !urls.contains(url) { urls.append(url) }
            let pairing = Pairing(
                base: api.http.base, urls: urls, pin: server.pin, device: paired.device, kind: key.kind,
                shownKind: paired.kind, attestNote: paired.note, server: server.name,
                params: info.params, pairedAt: .now, paramsReady: false)
            pairing.save()
            use(pairing)
            await fetchParams()
        } catch {
            fail("Could not register with \(server.name): \(describe(error))")
        }
    }

    /// Forgets the current connection and registers with `server` instead.
    func switchTo(_ server: KnownServer) async {
        guard !pairingBusy, !paramsBusy else { return }
        pairAgain()
        await register(with: server)
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
            pairStatus = denied ? Self.localNetworkHelp : "Looking for the laptop…"
            try await Task.sleep(for: .seconds(2))
        }
        if denied { throw CaptureError(Self.localNetworkHelp) }
        let reason = last.map { " (\(describe($0)))" } ?? ""
        throw CaptureError(
            "no laptop answered within \(seconds) s\(reason). Check that this phone is on the laptop's network and provenance serve runs")
    }

    static let localNetworkHelp = "Turn on Local Network in Settings › Capture, then pair again."

    private func fetchParams() async {
        guard var pairing, let api, !paramsBusy else { return }
        paramsBusy = true
        defer { paramsBusy = false }
        pairFailed = false
        pairStatus = "Downloading proof parameters…"
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
            fail("Proof parameters did not download: \(describe(error)).")
        }
    }

    func retryParams() async {
        await fetchParams()
    }

    private func waitForPairing() {
        pairFailed = false
        pairStatus = ""
    }

    private func fail(_ message: String) {
        pairStatus = message
        pairFailed = true
    }


    // MARK: 2. Shutter and seal

    func photoTaken(_ data: Data, at time: Date, fix injected: Fix? = nil) {
        retake()
        photos += 1
        let mine = photos
        capturing = true
        sealSteps = [.locate: .running]
        sealTask = Task { await seal(data, at: time, fix: injected, mine: mine) }
    }

    /// Fixes everything a later edit cannot change: the fix, the original,
    /// its fingerprint, the sealed position and the signature over both.
    private func seal(_ data: Data, at time: Date, fix injected: Fix?, mine: Int) async {
        let capturedAt = Timestamp.rfc3339(time)
        let native = Native.library
        let fixTask = Task {
            if let injected { return injected }
            return try await location.fix(at: time)
        }
        var step = SealStep.locate
        do {
            let photo = try await Task.detached { try Photo(data: data) }.value
            let original = try await Task.detached { try OriginalImage.render(photo) }.value
            guard mine == photos else { return }
            show(original)
            let fix = try await fixTask.value
            try Task.checkCancellation()
            sealSteps[.locate] = .done

            step = .fingerprint
            let fingerprint = try await run(step, mine) {
                let planes = original.channels()
                return try native.fingerprint(r: planes.r, g: planes.g, b: planes.b, size: original.size)
            }
            step = .seal
            let (secrets, envelope) = try await run(step, mine) {
                let hints = try native.hints(latitude: fix.latitude, longitude: fix.longitude)
                let secrets = LocationSecrets(
                    latitude: fix.latitude, longitude: fix.longitude, salt: LocationSecrets.freshSalt(), hints: hints)
                return (secrets, try native.envelope(secrets))
            }
            step = .sign
            guard let key else { throw CaptureError("this phone has no key yet") }
            let receipt = try await run(step, mine) {
                try Receipt.sign(fingerprint: fingerprint, envelope: envelope, capturedAt: capturedAt, key: key)
            }
            // A beat on the finished list before the review replaces it.
            try await Task.sleep(for: .milliseconds(600))
            guard mine == photos else { return }
            shot = Shot(photo: photo, capturedAt: capturedAt, fix: fix)
            sealed = Sealed(original: original, secrets: secrets, envelope: envelope, receipt: receipt)
            capturing = false
            scheduleLookup(immediately: true)
        } catch {
            fixTask.cancel()
            guard mine == photos, !Task.isCancelled else { return }
            sealSteps[step] = .failed
            sealFailure = Self.sealFailure(step, error)
            capturing = false
        }
    }

    /// Runs `work` off the main actor as one row of the seal list.
    private func run<T: Sendable>(_ step: SealStep, _ mine: Int, _ work: @escaping @Sendable () throws -> T) async throws -> T {
        try Task.checkCancellation()
        sealSteps[step] = .running
        let task = Task.detached(priority: .userInitiated, operation: work)
        let value = try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
        try Task.checkCancellation()
        if mine == photos { sealSteps[step] = .done }
        return value
    }

    private static func sealFailure(_ step: SealStep, _ error: any Error) -> Failure {
        let detail = describe(error)
        guard step == .locate else {
            return Failure(
                title: "Couldn't seal this photo", message: "Take the photo again.",
                detail: detail)
        }
        if detail.contains("blocked") || detail.contains("Allow location") {
            return Failure(
                title: "Location is off", message: "Turn it on in Settings.",
                detail: detail, action: .settings)
        }
        if detail.contains("approximate") {
            return Failure(
                title: "Precise location is off",
                message: "Turn it on in Settings.", detail: detail,
                action: .settings)
        }
        return Failure(
            title: "Couldn't find where you are",
            message: "Step outside or near a window and try again.",
            detail: detail)
    }

    /// Shows a freshly rendered original, with the crop on all of it.
    private func show(_ original: OriginalImage) {
        preview = original.image
        originalSize = original.size
        crop = .whole(original.size)
        turn = .zero
    }

    /// Drops the current photo and everything still running for it.
    func retake() {
        sealTask?.cancel()
        sendTask?.cancel()
        followTask?.cancel()
        lookupTask?.cancel()
        sealTask = nil
        sendTask = nil
        followTask = nil
        // Results still in flight for the old photo see a newer count and are dropped.
        lookups += 1
        photos += 1
        shot = nil
        preview = nil
        originalSize = Self.noOriginal
        sealed = nil
        sealSteps = [:]
        sealFailure = nil
        capturing = false
        crop = .whole(originalSize)
        turn = .zero
        photoChosen = false
        region = nil
        area = nil
        cellFailed = false
        cellPending = false
        sendSteps = [:]
        sendFailure = nil
        sending = false
        acceptedID = nil
        laptop = nil
        laptopNotice = nil
        storedOnly = false
        runningSince = [:]
        outcome = nil
    }

    // MARK: 3. Review

    /// The width of an average cell at the chosen resolution, in metres.
    var cellWidth: Double {
        Verdict.averageKm2[resolution].squareRoot() * 1000
    }

    /// The fix is coarser than the cell is wide, so the photo may be in a neighbouring cell.
    var looseFix: Bool {
        guard let fix = shot?.fix else { return false }
        return fix.accuracy * 2 >= cellWidth
    }

    private func scheduleLookup(immediately: Bool = false) {
        lookupTask?.cancel()
        cellPending = shot != nil
        lookupTask = Task {
            if !immediately { try? await Task.sleep(for: .milliseconds(120)) }
            guard !Task.isCancelled else { return }
            await lookupCell()
        }
    }

    /// The cell and its outline, both computed on the phone, so moving the
    /// size never tells the laptop which finer cells the person looked at.
    private func lookupCell() async {
        guard let fix = shot?.fix else {
            cellPending = false
            return
        }
        lookups += 1
        let mine = lookups
        let resolution = resolution
        let native = Native.library
        do {
            let (found, outline) = try await Task.detached {
                let found = try native.cell(latitude: fix.latitude, longitude: fix.longitude, resolution: resolution)
                return (found, try native.outline(found.cell))
            }.value
            guard mine == lookups else { return }
            region = found
            area = outline
            cellPending = false
            cellFailed = false
        } catch {
            guard mine == lookups else { return }
            if region?.resolution != resolution {
                region = nil
                area = nil
            }
            cellPending = false
            cellFailed = true
        }
    }

    // MARK: 4. Publish

    /// The region must be the one for the size on screen, not one still being replaced.
    var canPublish: Bool {
        sealed != nil && !cellPending && region?.resolution == resolution && !sending && acceptedID == nil
    }

    func publish() {
        guard canPublish else { return }
        sendFailure = nil
        sendTask = Task { await send() }
    }

    /// Back to the review after a failed publish, keeping the sealed photo.
    func backToReview() {
        guard acceptedID == nil, !sending else { return }
        sendFailure = nil
        sendSteps = [:]
        photoChosen = true
    }

    private func send() async {
        guard let sealed, let shot, let region, let api, canPublish else { return }
        sending = true
        sendSteps = [.prove: .running]
        let crop = crop
        let turn = turn
        let fix = shot.fix
        let native = Native.library
        var step = SendStep.prove
        do {
            let proof = try await sendRun(step) {
                try native.prove(params: ProofParams.directory, cell: region.cell, secrets: sealed.secrets)
            }
            guard proof.envelope == sealed.envelope else {
                throw CaptureError("the proof is for envelope \(Verdict.short(proof.envelope)), not \(Verdict.short(sealed.envelope))")
            }
            step = .upload
            let accepted = try await sendRun(step) {
                let upload = CaptureUpload(
                    originalPng: try sealed.original.png().base64EncodedString(), receipt: sealed.receipt, cell: region.cell,
                    resolution: region.resolution, crop: crop, turn: turn, accuracyMeters: fix.accuracy, locationProof: proof.proof,
                    location: .init(simulated: fix.simulated, accessory: fix.accessory))
                return try await api.upload(upload)
            }
            try Task.checkCancellation()
            acceptedID = accepted.id
            follow(accepted.id, api: api)
        } catch {
            if Task.isCancelled { return }
            sendSteps[step] = .failed
            sendFailure =
                step == .prove
                ? Failure(
                    title: "Couldn't prove the area", message: "Try again.",
                    detail: describe(error), action: .retry)
                : Failure(
                    title: "Your laptop didn't answer",
                    message: "Check it's on and on the same Wi-Fi.",
                    detail: describe(error), action: .retry)
            sending = false
        }
    }

    private func sendRun<T: Sendable>(_ step: SendStep, _ work: @escaping @Sendable () async throws -> T) async throws -> T {
        try Task.checkCancellation()
        sendSteps[step] = .running
        let task = Task.detached(priority: .userInitiated, operation: work)
        let value = try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
        try Task.checkCancellation()
        sendSteps[step] = .done
        return value
    }

    // MARK: 5. The laptop

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
                    laptopNotice = nil
                    outcome = .rejected(Failure(
                        title: "Still working on it",
                        message: "Your laptop is slower than usual. Check it.",
                        detail: "The capture \(id) is still running after \(Int(Self.followLimit / 60)) minutes."))
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
                        laptopNotice = nil
                        outcome = .rejected(Failure(
                            title: "Lost your laptop",
                            message: "Check it's on and on the same Wi-Fi.",
                            detail: "\(describe(error)). Capture \(id)."))
                        return
                    }
                    laptopNotice = "Reconnecting…"
                    try? await Task.sleep(for: .milliseconds(1500))
                    continue
                }
                guard let current = status, !Task.isCancelled else { return }
                if current.phase == .stored {
                    storedOnly = true
                    laptopNotice = nil
                    return
                }
                laptop = current
                noteRunning()
                if !current.phase.isRunning { break }
                laptopNotice = current.phase == .queued ? "Waiting for your laptop" : nil
                try? await Task.sleep(for: .milliseconds(700))
            }
            guard let status, !Task.isCancelled else { return }
            laptopNotice = nil
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
            let rejected = status.phase == .rejected
            outcome = .rejected(Failure(
                title: rejected ? "This photo didn't pass the checks" : "Publishing stopped",
                message: rejected
                    ? "It wasn't published. Take the photo again."
                    : "Something went wrong on your laptop. Take the photo again.",
                detail: status.error ?? "unknown error"))
            return
        }
        do {
            let png = try await api.signedPNG(status.id)
            let file = try gallery.add(SavedPhoto(id: status.id, capturedAt: claim.capturedAt, cell: claim.cell), png: png)
            guard !Task.isCancelled else { return }
            outcome = .accepted(claim, image: png, file: file)
        } catch {
            guard !Task.isCancelled else { return }
            outcome = .rejected(Failure(
                title: "Couldn't bring the file back",
                message: "It's verified and saved on your laptop.",
                detail: describe(error)))
        }
    }

    /// The published cell's outline: the one already drawn when it is the chosen cell.
    var publishedArea: Outline? {
        guard case .accepted(let claim, _, _) = outcome else { return nil }
        if let area, area.cell == claim.cell { return area }
        return try? Native.library.outline(claim.cell)
    }
}

#if DEBUG
    /// States that are hard to reach on purpose, for checking each screen in
    /// the Simulator: launch with `-scenario <name>`.
    extension CaptureFlow {
        func applyScenario() {
            guard let name = UserDefaults.standard.string(forKey: "scenario"),
                let url = Bundle.main.url(forResource: "sample", withExtension: "jpg"),
                let data = try? Data(contentsOf: url)
            else { return }
            let original = (try? Photo(data: data)).flatMap { try? OriginalImage.render($0) }
            switch name {
            case "seal":
                // A real seal of the sample photo at Universidad Torcuato Di Tella.
                let fix = Fix(CLLocation(
                    coordinate: CLLocationCoordinate2D(latitude: -34.5478, longitude: -58.4462), altitude: 0,
                    horizontalAccuracy: 8, verticalAccuracy: 8, timestamp: .now))
                photoTaken(data, at: .now, fix: fix)
            case "sealFailed":
                if let original { show(original) }
                sealSteps = [.locate: .failed]
                sealFailure = Self.sealFailure(.locate, CaptureError("No GPS fix. Try again."))
            case "sendFailed":
                if let original { show(original) }
                sendSteps = [.prove: .done, .upload: .failed]
                sendFailure = Failure(
                    title: "Your laptop didn't answer",
                    message: "Check it's on and on the same Wi-Fi.",
                    detail: "Could not connect to the server.", action: .retry)
            case "rejected":
                outcome = .rejected(Failure(
                    title: "This photo didn't pass the checks",
                    message: "It wasn't published. Take the photo again.",
                    detail: "location: the proof does not verify for cell 87c2e3020ffffff"))
            case "lost":
                outcome = .rejected(Failure(
                    title: "Lost your laptop",
                    message: "Check it's on and on the same Wi-Fi.",
                    detail: "Could not connect to the server. Capture 20261102-124501-3f2a."))
            case "stored":
                acceptedID = "20261102-124501-3f2a"
                storedOnly = true
            case "pairFailed":
                use(nil)
                fail("Could not pair: no laptop answered within 10 s (Could not connect to the server.).")
            default:
                break
            }
        }
    }
#endif
