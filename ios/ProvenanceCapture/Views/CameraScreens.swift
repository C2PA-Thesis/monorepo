import PhotosUI
import SwiftUI

/// The home screen: the viewfinder shows the whole frame, which becomes the
/// photo. The time is read when the shutter fires.
struct CameraHome: View {
    @Bindable var flow: CaptureFlow
    @State private var camera = Camera()
    @State private var cameraError: String?
    @State private var cameraBlocked = false
    @State private var ready = CaptureFlow.isSimulator
    @State private var shooting = false
    @State private var flash = false
    @State private var showConnection = false
    @State private var showGallery = false
    @State private var picked: PhotosPickerItem?
    @State private var flipping = false

    var body: some View {
        VStack(spacing: 0) {
            topBar
            viewfinder
            Spacer(minLength: 0)
            bottomBar
        }
        .background(Color.black.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .task {
            guard !CaptureFlow.isSimulator else { return }
            do {
                try await camera.start(.photo)
                ready = true
            } catch {
                cameraBlocked = (error as? CaptureError) == Camera.blocked
                cameraError = cameraBlocked ? "Camera access is off for Capture." : describe(error)
            }
        }
        .onDisappear { camera.stop() }
        .sheet(isPresented: $showConnection) { ConnectionSheet(flow: flow) }
        .sheet(isPresented: $showGallery) { GalleryScreen(flow: flow) }
        .onChange(of: picked) { _, item in
            guard let item else { return }
            let time = Date.now
            Task {
                if let data = try? await item.loadTransferable(type: Data.self) { flow.photoTaken(data, at: time) }
                picked = nil
            }
        }
    }

    private var topBar: some View {
        HStack {
            LocationChip(location: flow.location)
            Spacer()
            Button { showConnection = true } label: {
                Image(systemName: "laptopcomputer").font(.system(size: 15, weight: .medium))
                    .frame(width: 40, height: 40)
                    .background(.white.opacity(0.12), in: Circle())
                    .overlay(alignment: .topTrailing) {
                        Circle().fill(Theme.verify).frame(width: 8, height: 8)
                            .overlay(Circle().strokeBorder(.black, lineWidth: 1.5))
                    }
            }
            .foregroundStyle(.white)
            .accessibilityLabel("Laptop connection")
        }
        .padding(.horizontal, 16)
        .frame(height: 60)
    }

    /// A 3:4 preview, full width: the .photo preset's 4:3 frame held upright.
    private var viewfinder: some View {
        GeometryReader { geometry in
            ZStack {
                #if targetEnvironment(simulator)
                    SimulatorFeed()
                #else
                    CameraPreview(session: camera.session, gravity: .resizeAspect)
                #endif
                if let cameraError {
                    VStack(spacing: 14) {
                        Text(cameraError).font(Typeface.callout).foregroundStyle(.white).multilineTextAlignment(.center)
                        if cameraBlocked { OpenSettingsButton(style: PillButtonStyle(kind: .light)).frame(width: 200) }
                    }
                    .padding(24)
                }
                Color.white.opacity(flash ? 0.85 : 0).allowsHitTesting(false)
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .aspectRatio(3 / 4, contentMode: .fit)
        .clipped()
    }

    private var bottomBar: some View {
        HStack {
            lastThumbnail.frame(width: 56, height: 56)
            Spacer()
            Button(action: shoot) {
                ZStack {
                    Circle().strokeBorder(.white, lineWidth: 4)
                    Circle().fill(.white).padding(8).scaleEffect(shooting ? 0.86 : 1)
                }
                .frame(width: 78, height: 78)
                .opacity(canShoot ? 1 : 0.4)
            }
            .disabled(!canShoot)
            .accessibilityLabel("Take the photo")
            Spacer()
            libraryButton.frame(width: 56, height: 56)
        }
        .padding(.horizontal, 28)
        .padding(.vertical, 24)
    }

    @ViewBuilder private var lastThumbnail: some View {
        Button { showGallery = true } label: {
            if let photo = flow.gallery.photos.first, let image = flow.gallery.image(photo) {
                Image(uiImage: image).resizable().scaledToFill()
                    .frame(width: 48, height: 48)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.white.opacity(0.7), lineWidth: 1.5))
                    .overlay(alignment: .topTrailing) { VerifiedMark(size: 18).offset(x: 6, y: -6) }
            } else {
                Image(systemName: "square.grid.2x2").font(.system(size: 18, weight: .medium))
                    .frame(width: 48, height: 48)
                    .background(.white.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                    .foregroundStyle(.white)
            }
        }
        .accessibilityLabel("Your photos")
    }

    @ViewBuilder private var libraryButton: some View {
        #if targetEnvironment(simulator)
            // The Simulator has no camera; a library photo stands in for the shutter.
            PhotosPicker(selection: $picked, matching: .images) {
                Image(systemName: "photo.on.rectangle").font(.system(size: 18, weight: .medium))
                    .frame(width: 48, height: 48)
                    .background(.white.opacity(0.12), in: Circle())
                    .foregroundStyle(.white)
            }
            .accessibilityLabel("Use a photo from the library")
        #else
            Button(action: flip) {
                Image(systemName: "arrow.triangle.2.circlepath.camera").font(.system(size: 18, weight: .medium))
                    .frame(width: 48, height: 48)
                    .background(.white.opacity(0.12), in: Circle())
                    .foregroundStyle(.white)
            }
            .disabled(!canShoot)
            .opacity(canShoot ? 1 : 0.4)
            .accessibilityLabel("Switch camera")
        #endif
    }

    private func flip() {
        guard canShoot, !flipping else { return }
        flipping = true
        Task {
            defer { flipping = false }
            do {
                _ = try await camera.flip()
            } catch {
                cameraError = describe(error)
            }
        }
    }

    private var canShoot: Bool { ready && !shooting && !flipping }

    private func shoot() {
        guard canShoot else { return }
        shooting = true
        let time = Date.now
        withAnimation(.easeOut(duration: 0.08)) { flash = true }
        withAnimation(.easeIn(duration: 0.3).delay(0.08)) { flash = false }
        Task {
            defer { shooting = false }
            #if targetEnvironment(simulator)
                if let url = Bundle.main.url(forResource: "sample", withExtension: "jpg"),
                    let data = try? Data(contentsOf: url)
                {
                    try? await Task.sleep(for: .milliseconds(250))
                    flow.photoTaken(data, at: time)
                }
            #else
                do {
                    flow.photoTaken(try await camera.takePhoto(), at: time)
                } catch {
                    cameraError = describe(error)
                }
            #endif
        }
    }
}

/// The location state, over the viewfinder.
private struct LocationChip: View {
    let location: LocationService

    var body: some View {
        Group {
            if let url = settingsURL {
                Link(destination: url) { label }
            } else {
                label
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var settingsURL: URL? {
        if case .error = location.status, location.access != .granted { return URL(string: UIApplication.openSettingsURLString) }
        return nil
    }

    private var label: some View {
        HStack(spacing: 8) {
            switch location.status {
            case .ok:
                Circle().fill(Theme.verify).frame(width: 7, height: 7)
                    .overlay(Circle().stroke(Theme.verify.opacity(0.3), lineWidth: 3))
                Text("Location ready").foregroundStyle(.white)
            case .waiting:
                Spinner(color: .white).frame(width: 12, height: 12)
                Text("Finding your location…").foregroundStyle(.white)
            case .error(let text, _):
                Circle().fill(Theme.warn).frame(width: 7, height: 7)
                Text(text).foregroundStyle(.white)
            }
        }
        .font(Typeface.sans(13.5, .semibold, relativeTo: .footnote))
        .padding(.horizontal, 14)
        .frame(height: 36)
        .background(.white.opacity(0.12), in: Capsule())
    }
}

#if targetEnvironment(simulator)
    /// The bundled sample photo in place of a camera feed. Fit, not fill, so
    /// all of what gets sealed shows.
    private struct SimulatorFeed: View {
        var body: some View {
            if let url = Bundle.main.url(forResource: "sample", withExtension: "jpg"), let image = UIImage(contentsOfFile: url.path()) {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                Color(white: 0.12)
            }
        }
    }
#endif

/// Which laptop this iPhone sends to, and a way to connect to another.
struct ConnectionSheet: View {
    let flow: CaptureFlow
    @State private var confirming = false
    private let servers = KnownServers.available
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            HStack {
                Text("Connection").font(Typeface.title).tight(26).foregroundStyle(Theme.ink)
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark").font(.system(size: 13, weight: .bold)).foregroundStyle(Theme.soft)
                        .frame(width: 32, height: 32).background(Theme.surface, in: Circle())
                }
                .accessibilityLabel("Close")
            }
            VStack(spacing: 0) {
                row(flow.pairing?.server == nil ? "Laptop" : "Server", value: connectionText, ok: flow.isPaired)
                Divider().overlay(Theme.line)
                if let pairing = flow.pairing {
                    row("Key", value: keyText(pairing), ok: pairing.shownKind?.hasPrefix("attested") == true || pairing.kind == .software)
                    Divider().overlay(Theme.line)
                }
                row("Location", value: locationText, ok: flow.location.access == .granted)
            }
            .padding(.horizontal, 16)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.md))
            Spacer(minLength: 0)
            Button(servers.isEmpty ? "Connect to another laptop" : "Connect somewhere else") { confirming = true }
                .buttonStyle(.ghost)
        }
        .padding(24)
        .background(Theme.paper.ignoresSafeArea())
        .presentationDetents([.medium])
        .presentationCornerRadius(Theme.Radius.lg)
        .confirmationDialog("Connect somewhere else?", isPresented: $confirming, titleVisibility: .visible) {
            ForEach(servers.filter { $0.name != flow.pairing?.server }) { server in
                Button("Switch to \(server.name)") {
                    dismiss()
                    Task { await flow.switchTo(server) }
                }
            }
            Button("Disconnect and scan again", role: .destructive) {
                dismiss()
                flow.pairAgain()
            }
        } message: {
            Text("This iPhone forgets the current connection.")
        }
    }

    /// What the laptop recorded about this key, and why it is not attested when it is not.
    private func keyText(_ pairing: Pairing) -> String {
        let shown = pairing.shownKind ?? pairing.kind.rawValue
        let text: String = switch shown {
        case let attested where attested.hasPrefix("attested"):
            "Secure Enclave, attested by Apple" + (attested.contains("(development)") ? " (development)" : "")
        case "declared: secure-enclave": "Secure Enclave, declared by the app"
        case "software": "Software key (Simulator)"
        default: shown
        }
        guard let note = pairing.attestNote, !shown.hasPrefix("attested") else { return text }
        return "\(text). \(note.prefix(1).uppercased() + note.dropFirst())"
    }

    private var connectionText: String {
        guard let pairing = flow.pairing else { return "Not connected" }
        let since = "since \(pairing.pairedAt.formatted(date: .abbreviated, time: .omitted))"
        return pairing.server.map { "\($0), registered \(since)" } ?? "Connected \(since)"
    }

    private var locationText: String {
        switch flow.location.access {
        case .granted: "Precise"
        case .approximate: "Approximate. Turn on Precise Location in Settings"
        case .denied: "Off"
        case .notAsked: "Not asked yet"
        }
    }

    private func row(_ label: String, value: String, ok: Bool) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Image(systemName: ok ? "checkmark.circle.fill" : "circle").foregroundStyle(ok ? Theme.verify : Theme.faint)
                .font(.system(size: 15))
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(Typeface.headline).foregroundStyle(Theme.ink)
                Text(value).font(Typeface.sans(14, relativeTo: .subheadline)).foregroundStyle(Theme.soft)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 14)
        .accessibilityElement(children: .combine)
    }
}

/// Reads the pairing QR code the laptop shows.
struct ScannerScreen: View {
    let scanned: (String) -> Void
    let cancel: () -> Void
    @State private var camera = Camera()
    @State private var error: String?
    @State private var done = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            CameraPreview(session: camera.session).ignoresSafeArea()
            RoundedRectangle(cornerRadius: Theme.Radius.lg).strokeBorder(.white, lineWidth: 3).frame(width: 248, height: 248)
                .allowsHitTesting(false)
            VStack {
                HStack {
                    Button(action: cancel) {
                        Image(systemName: "xmark").font(.system(size: 15, weight: .semibold))
                            .frame(width: 40, height: 40).background(.white.opacity(0.14), in: Circle())
                    }
                    .foregroundStyle(.white)
                    .accessibilityLabel("Cancel")
                    Spacer()
                }
                .padding(16)
                Spacer()
                Text(error ?? "Point at the code on your laptop")
                    .font(Typeface.callout).foregroundStyle(.white).multilineTextAlignment(.center)
                    .padding(.horizontal, 18).padding(.vertical, 12)
                    .background(.black.opacity(0.55), in: Capsule())
                    .padding(.bottom, 48)
                    .padding(.horizontal, 24)
            }
        }
        .preferredColorScheme(.dark)
        .task {
            do {
                try await camera.start(.qr) { code in
                    guard !done else { return }
                    done = true
                    camera.stop()
                    scanned(code)
                }
            } catch {
                self.error = describe(error)
            }
        }
        .onDisappear { camera.stop() }
    }
}
