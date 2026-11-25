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
                try await camera.start()
                ready = true
            } catch {
                cameraBlocked = (error as? CaptureError) == Camera.blocked
                cameraError = cameraBlocked ? "Camera access is off for Capture." : describe(error)
            }
        }
        .onDisappear { camera.stop() }
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
                Circle().strokeBorder(.white, lineWidth: 4)
                    .frame(width: 78, height: 78)
                    .opacity(canShoot ? 1 : 0.4)
            }
            .buttonStyle(ShutterStyle(shooting: shooting))
            .disabled(!canShoot)
            .sensoryFeedback(.impact(weight: .medium), trigger: shooting) { _, new in new }
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
                    .clipShape(Circle())
                    .overlay(Circle().strokeBorder(.white.opacity(0.7), lineWidth: 1.5))
                    .overlay(alignment: .topTrailing) { VerifiedMark(size: 18).offset(x: 4, y: -4) }
                    .id(photo.id)
                    .transition(.scale(scale: 0.5).combined(with: .opacity))
            } else {
                CameraIcon(name: "square.grid.2x2")
            }
        }
        .animation(.spring(duration: 0.45, bounce: 0.3), value: flow.gallery.photos.first?.id)
        .accessibilityLabel("Your photos")
    }

    @ViewBuilder private var libraryButton: some View {
        #if targetEnvironment(simulator)
            // The Simulator has no camera; a library photo stands in for the shutter.
            PhotosPicker(selection: $picked, matching: .images) {
                CameraIcon(name: "photo.on.rectangle")
            }
            .accessibilityLabel("Use a photo from the library")
        #else
            Button(action: flip) {
                CameraIcon(name: "arrow.triangle.2.circlepath.camera")
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
                // The camera plays its own shutter sound on a device.
                Sound.shutter.play()
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

/// The shutter: the inner disc sinks while pressed and while the photo is taken.
private struct ShutterStyle: ButtonStyle {
    let shooting: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .overlay {
                Circle().fill(.white).padding(8)
                    .scaleEffect(configuration.isPressed ? 0.88 : shooting ? 0.92 : 1)
                    .animation(.spring(duration: 0.25, bounce: 0.3), value: configuration.isPressed)
                    .animation(.spring(duration: 0.25, bounce: 0.3), value: shooting)
            }
    }
}

/// A round icon button on the camera's black bar.
private struct CameraIcon: View {
    let name: String

    var body: some View {
        Image(systemName: name).font(.system(size: 18, weight: .medium))
            .frame(width: 48, height: 48)
            .background(.white.opacity(0.12), in: Circle())
            .foregroundStyle(.white)
    }
}

/// The location state, over the viewfinder. Ready means a fix recent enough
/// for the shutter to use, not just that one arrived at some point.
private struct LocationChip: View {
    let location: LocationService

    var body: some View {
        // Re-read every second: a fix goes stale without any new event.
        TimelineView(.periodic(from: .now, by: 1)) { context in
            chip(fresh: location.isFresh(at: context.date))
        }
    }

    private func chip(fresh: Bool) -> some View {
        Group {
            if let url = settingsURL {
                Link(destination: url) { label(fresh: fresh) }
            } else {
                label(fresh: fresh)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var settingsURL: URL? {
        if case .error = location.status, location.access != .granted { return URL(string: UIApplication.openSettingsURLString) }
        return nil
    }

    private func label(fresh: Bool) -> some View {
        HStack(spacing: 8) {
            switch location.status {
            case .ok where fresh:
                Circle().fill(Theme.verify).frame(width: 7, height: 7)
                    .overlay(Circle().stroke(Theme.verify.opacity(0.3), lineWidth: 3))
                    .transition(.scale.combined(with: .opacity))
                Text("Location ready").foregroundStyle(.white)
                    .transition(.opacity)
            case .ok, .waiting:
                Spinner(color: .white).frame(width: 12, height: 12)
                    .transition(.opacity)
                Text("Finding your location…").foregroundStyle(.white)
                    .transition(.opacity)
            case .error(let text, _):
                Circle().fill(Theme.warn).frame(width: 7, height: 7)
                Text(text).foregroundStyle(.white)
            }
        }
        .font(Typeface.sans(13.5, .semibold, relativeTo: .footnote))
        .padding(.horizontal, 14)
        .frame(height: 36)
        .background(.white.opacity(0.12), in: Capsule())
        .animation(Theme.ease, value: ready(fresh))
    }

    private func ready(_ fresh: Bool) -> Bool {
        if case .ok = location.status { return fresh }
        return false
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
