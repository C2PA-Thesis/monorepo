@preconcurrency import AVFoundation
import SwiftUI

/// The app's own capture session. Photos come straight from the sensor, never
/// from the library, so the app knows when the shutter fired.
final class Camera: NSObject, @unchecked Sendable {
    // The session and its outputs are only touched on `queue`.
    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "io.github.c2pa-thesis.capture.camera")
    private let photoOutput = AVCapturePhotoOutput()
    private let metadataOutput = AVCaptureMetadataOutput()
    private var configured = false
    private var input: AVCaptureDeviceInput?
    private var rotation: AVCaptureDevice.RotationCoordinator?
    private var pending: CheckedContinuation<Data, any Error>?
    private var onCode: (@MainActor @Sendable (String) -> Void)?

    enum Mode { case photo, qr }

    static func authorize() async throws {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: return
        case .notDetermined:
            if await AVCaptureDevice.requestAccess(for: .video) { return }
        default: break
        }
        throw blocked
    }

    static let blocked = CaptureError("Camera access is blocked. Allow it in Settings › Privacy & Security › Camera.")

    /// Starts the back camera for photos, or for QR codes calling `onCode` with each payload.
    func start(_ mode: Mode, onCode: (@MainActor @Sendable (String) -> Void)? = nil) async throws {
        try await Self.authorize()
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            queue.async {
                do {
                    try self.configure(mode)
                    self.onCode = onCode
                    if !self.session.isRunning { self.session.startRunning() }
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Switches between the back and the front camera and returns the one now in use.
    func flip() async throws -> AVCaptureDevice.Position {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do {
                    continuation.resume(returning: try self.switchCamera())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    func stop() {
        queue.async {
            if self.session.isRunning { self.session.stopRunning() }
        }
    }

    /// Takes one photo and returns its encoded bytes.
    func takePhoto() async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                guard self.pending == nil else {
                    continuation.resume(throwing: CaptureError("a photo is already being taken"))
                    return
                }
                // capturePhoto raises an Objective-C exception, which Swift cannot catch,
                // when the output has no active video connection.
                guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
                    continuation.resume(throwing: Self.blocked)
                    return
                }
                guard self.configured, self.session.isRunning, let connection = self.photoOutput.connection(with: .video),
                    connection.isEnabled, connection.isActive
                else {
                    continuation.resume(throwing: CaptureError("the camera is not running. Close it and open it again"))
                    return
                }
                self.pending = continuation
                let settings = AVCapturePhotoSettings(format: [AVVideoCodecKey: AVVideoCodecType.jpeg])
                settings.flashMode = .off
                // Records how the phone was held, so the photo decodes upright.
                if let rotation = self.rotation {
                    let angle = rotation.videoRotationAngleForHorizonLevelCapture
                    if connection.isVideoRotationAngleSupported(angle) { connection.videoRotationAngle = angle }
                }
                self.photoOutput.capturePhoto(with: settings, delegate: self)
            }
        }
    }

    private func configure(_ mode: Mode) throws {
        guard !configured else { return }
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back) else {
            throw CaptureError("this device has no back camera")
        }
        let input = try AVCaptureDeviceInput(device: device)
        self.input = input
        session.beginConfiguration()
        defer { session.commitConfiguration() }
        session.sessionPreset = mode == .photo ? .photo : .high
        guard session.canAddInput(input) else { throw CaptureError("the camera is busy") }
        session.addInput(input)
        switch mode {
        case .photo where session.canAddOutput(photoOutput):
            session.addOutput(photoOutput)
        case .qr where session.canAddOutput(metadataOutput):
            session.addOutput(metadataOutput)
            metadataOutput.setMetadataObjectsDelegate(self, queue: queue)
            metadataOutput.metadataObjectTypes = [.qr]
        default:
            // Leaves the session empty, so the next start begins again from the input.
            session.removeInput(input)
            self.input = nil
            throw CaptureError(mode == .photo ? "the camera cannot take photos" : "the camera cannot read QR codes")
        }
        rotation = AVCaptureDevice.RotationCoordinator(device: device, previewLayer: nil)
        configured = true
    }

    private func switchCamera() throws -> AVCaptureDevice.Position {
        guard configured, let current = input, pending == nil else {
            throw CaptureError("the camera cannot switch right now")
        }
        let position: AVCaptureDevice.Position = current.device.position == .front ? .back : .front
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: position) else {
            throw CaptureError(position == .front ? "this device has no front camera" : "this device has no back camera")
        }
        let next = try AVCaptureDeviceInput(device: device)
        session.beginConfiguration()
        defer { session.commitConfiguration() }
        session.removeInput(current)
        guard session.canAddInput(next) else {
            session.addInput(current)
            throw CaptureError("the camera is busy")
        }
        session.addInput(next)
        input = next
        rotation = AVCaptureDevice.RotationCoordinator(device: device, previewLayer: nil)
        return position
    }
}

extension Camera: AVCapturePhotoCaptureDelegate {
    func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: (any Error)?) {
        queue.async {
            let continuation = self.pending
            self.pending = nil
            if let error {
                continuation?.resume(throwing: error)
            } else if let data = photo.fileDataRepresentation() {
                continuation?.resume(returning: data)
            } else {
                continuation?.resume(throwing: CaptureError("the camera returned no image"))
            }
        }
    }
}

extension Camera: AVCaptureMetadataOutputObjectsDelegate {
    func metadataOutput(
        _ output: AVCaptureMetadataOutput, didOutput metadataObjects: [AVMetadataObject], from connection: AVCaptureConnection
    ) {
        let codes = metadataObjects.compactMap { ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }
        guard let code = codes.first, let onCode else { return }
        Task { @MainActor in onCode(code) }
    }
}

/// The live camera image.
struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession
    /// Aspect fit shows the whole frame, all of which the photo keeps.
    var gravity = AVLayerVideoGravity.resizeAspectFill

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var preview: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.preview.session = session
        view.preview.videoGravity = gravity
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {}
}
