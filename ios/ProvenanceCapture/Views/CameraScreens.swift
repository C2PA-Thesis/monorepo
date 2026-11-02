import SwiftUI

/// The viewfinder and the shutter. The time is read when the shutter fires.
struct CameraScreen: View {
    let taken: (Data, Date) -> Void
    let cancel: () -> Void
    @State private var camera = Camera()
    @State private var error: String?
    @State private var busy = false
    /// The session runs with the photo output attached. Until then the shutter does nothing.
    @State private var ready = false
    @State private var blocked = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            CameraPreview(session: camera.session, gravity: .resizeAspect).ignoresSafeArea()
            // The 2:1 window the frame step starts with (FramingWindow at 0.5) over the
            // aspect-fit preview. The .photo preset is 4:3, so upright the photo is 3 wide by 4 tall.
            GeometryReader { geometry in
                let size = geometry.size
                let photo = FramingWindow(photoWidth: min(size.width, size.height * 3 / 4), photoHeight: min(size.height, size.width * 4 / 3))
                Rectangle().stroke(.white.opacity(0.7), style: StrokeStyle(lineWidth: 1, dash: [6, 4]))
                    .frame(width: photo.width, height: photo.height)
                    .position(x: size.width / 2, y: size.height / 2)
            }
            .ignoresSafeArea()
            .allowsHitTesting(false)
            VStack {
                HStack {
                    Button("Cancel", action: cancel).foregroundStyle(.white).padding()
                    Spacer()
                }
                Spacer()
                if let error {
                    Text(error).font(.footnote).foregroundStyle(.white).padding(10)
                        .background(.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 8))
                        .padding(.horizontal, 20)
                    if blocked { SettingsLink() }
                }
                Button(action: shoot) {
                    Circle().stroke(.white, lineWidth: 4).frame(width: 74, height: 74)
                        .overlay(Circle().fill(.white).padding(8).opacity(busy || !ready ? 0.4 : 1))
                }
                .disabled(busy || !ready)
                .accessibilityLabel("Take the photo")
                .padding(.bottom, 30)
            }
        }
        .task {
            do {
                try await camera.start(.photo)
                ready = true
            } catch {
                blocked = (error as? CaptureError) == Camera.blocked
                self.error = describe(error)
            }
        }
        .onDisappear { camera.stop() }
    }

    private func shoot() {
        guard ready, !busy else { return }
        busy = true
        let time = Date.now
        Task {
            do {
                let data = try await camera.takePhoto()
                taken(data, time)
            } catch {
                self.error = describe(error)
            }
            busy = false
        }
    }
}

/// Opens this app's page in Settings, where camera access is turned back on.
private struct SettingsLink: View {
    var body: some View {
        if let url = URL(string: UIApplication.openSettingsURLString) {
            Link("Open Settings", destination: url).font(.footnote.weight(.semibold)).foregroundStyle(.white).padding(8)
        }
    }
}

/// Reads the pairing QR code the presenter page shows.
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
            RoundedRectangle(cornerRadius: 16).stroke(.white, lineWidth: 3).frame(width: 240, height: 240)
                .allowsHitTesting(false)
            VStack {
                HStack {
                    Button("Cancel", action: cancel).foregroundStyle(.white).padding()
                    Spacer()
                }
                Spacer()
                Text(error ?? "Point at the app QR on the laptop")
                    .font(.footnote).foregroundStyle(.white).multilineTextAlignment(.center).padding(12)
                    .background(.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 8))
                    .padding(.bottom, 40)
                    .padding(.horizontal, 20)
            }
        }
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
