import PhotosUI
import SwiftUI

struct PairStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        VStack(spacing: 0) {
            StepCard(number: 1, title: "Pair this phone", summary: flow.pairSummary, state: flow.pairState) {
                VStack(alignment: .leading, spacing: 12) {
                    Text(flow.pairStatus)
                        .font(.subheadline)
                        .foregroundStyle(flow.pairFailed ? Theme.bad : Theme.muted)
                        .fixedSize(horizontal: false, vertical: true)
                    if let progress = flow.paramsStatus {
                        HStack(spacing: 8) {
                            ProgressView().controlSize(.small)
                            Text(progress).font(.footnote).foregroundStyle(Theme.muted)
                        }
                    }
                    if flow.pairing != nil {
                        if !flow.isPaired && !flow.paramsBusy {
                            Button("Download the parameters again") { Task { await flow.retryParams() } }
                                .buttonStyle(WideButtonStyle())
                            // The download can fail for good: the laptop's parameters, certificate
                            // or address changed since the pairing was saved.
                            Button("Pair again") { flow.pairAgain() }
                                .buttonStyle(WideButtonStyle(quiet: true))
                        }
                    } else {
                        actions
                    }
                    Hint(
                        "The laptop shows a QR code for this app. Scanning it pairs this phone and pins the laptop's certificate. Pairing tells the laptop to trust this phone's key, which never leaves "
                            + (CaptureFlow.isSimulator ? "the Keychain." : "the Secure Enclave."))
                }
                .disabled(flow.pairingBusy)
            }
            if flow.isPaired {
                HStack {
                    Spacer()
                    Button("Pair again") { flow.pairAgain() }
                        .font(.footnote)
                        .padding(.top, 6)
                }
            }
        }
    }

    @ViewBuilder private var actions: some View {
        if CaptureFlow.isSimulator {
            Button("Pair with provenance serve on this Mac") { Task { await flow.fetchFromMac() } }
                .buttonStyle(WideButtonStyle())
        } else {
            Button {
                flow.showScanner = true
            } label: {
                Label("Scan the pairing QR code", systemImage: "qrcode.viewfinder")
            }
            .buttonStyle(WideButtonStyle())
        }
        DisclosureGroup("Paste the pairing code instead") {
            VStack(spacing: 8) {
                TextField("{\"v\":1,\"urls\":[…]}", text: $flow.pasted, axis: .vertical)
                    .font(.caption.monospaced())
                    .lineLimit(3...6)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(8)
                    .background(Theme.surface2, in: RoundedRectangle(cornerRadius: 8))
                Button("Pair") { Task { await flow.pairPasted() } }
                    .buttonStyle(WideButtonStyle(quiet: true))
                    .disabled(flow.pasted.isEmpty)
            }
            .padding(.top, 8)
        }
        .font(.footnote)
    }
}

struct PhotoStep: View {
    @Bindable var flow: CaptureFlow
    @State private var picked: PhotosPickerItem?

    var body: some View {
        StepCard(number: 2, title: "Take a photo", summary: flow.photoSummary, state: flow.photoState) {
            VStack(alignment: .leading, spacing: 12) {
                GPSBadge(status: flow.location.status)
                if case .error(_, let help?) = flow.location.status { Hint(help) }
                #if targetEnvironment(simulator)
                    Button {
                        Task { await flow.useSample() }
                    } label: {
                        Label("Use the sample photo", systemImage: "photo")
                    }
                    .buttonStyle(WideButtonStyle())
                    PhotosPicker(selection: $picked, matching: .images) {
                        Label("Choose a photo", systemImage: "photo.on.rectangle")
                    }
                    .buttonStyle(WideButtonStyle(quiet: true))
                    Hint("The Simulator has no camera. Its location comes from Xcode (Features › Location or a GPX file) and is marked simulated.")
                #else
                    Button {
                        flow.openCamera()
                    } label: {
                        Label("Open the camera", systemImage: "camera")
                    }
                    .buttonStyle(WideButtonStyle())
                    Hint("The coordinate and the time are read when the shutter fires.")
                #endif
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            guard flow.photoState == .done, !flow.sending else { return }
            if CaptureFlow.isSimulator { flow.retake() } else { flow.openCamera() }
        }
        .id("photo")
        .onChange(of: picked) { _, item in
            guard let item else { return }
            let time = Date.now
            Task {
                if let data = try? await item.loadTransferable(type: Data.self) {
                    await flow.photoTaken(data, at: time)
                }
                picked = nil
            }
        }
    }
}

struct GPSBadge: View {
    let status: LocationService.Status

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(color).frame(width: 9, height: 9)
            Text(text).font(.subheadline).foregroundStyle(isError ? Theme.bad : Theme.ink)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(isError ? Theme.badSoft : Theme.surface2, in: Capsule())
    }

    private var text: String {
        switch status {
        case .waiting(let text), .ok(let text), .error(let text, _): text
        }
    }

    private var isError: Bool { if case .error = status { true } else { false } }

    private var color: Color {
        switch status {
        case .waiting: Theme.faint
        case .ok: Theme.ok
        case .error: Theme.bad
        }
    }
}
