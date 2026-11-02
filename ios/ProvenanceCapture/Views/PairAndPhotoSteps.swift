import PhotosUI
import SwiftUI

struct PairStep: View {
    @Bindable var flow: CaptureFlow
    @State private var confirming = false

    var body: some View {
        StepCard(
            number: 1, title: "Pair", value: flow.pairSummary, state: flow.pairState,
            reopen: { confirming = true }, reopenHint: "Pairs this phone again"
        ) {
            VStack(alignment: .leading, spacing: 12) {
                status
                if flow.pairing != nil {
                    if !flow.isPaired && !flow.paramsBusy {
                        Button("Retry") { Task { await flow.retryParams() } }
                            .buttonStyle(.primary)
                        // The download can fail for good: the laptop's parameters, certificate
                        // or address changed since the pairing was saved.
                        Button("Pair again") { flow.pairAgain() }
                            .buttonStyle(.secondary)
                    }
                } else if !flow.pairWorking {
                    actions
                }
                if flow.pairNeedsSettings { OpenSettingsButton() }
            }
            .disabled(flow.pairingBusy)
        }
        .confirmationDialog("Pair this phone again?", isPresented: $confirming, titleVisibility: .visible) {
            Button("Pair again", role: .destructive) { flow.pairAgain() }
        }
    }

    @ViewBuilder private var status: some View {
        if flow.pairWorking {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small).tint(Theme.ink)
                Text(flow.paramsStatus ?? flow.pairStatus).font(.subheadline).foregroundStyle(Theme.ink2)
            }
        } else if !flow.pairStatus.isEmpty {
            Note(flow.pairStatus, tone: flow.pairFailed ? Theme.bad : Theme.muted)
        }
    }

    @ViewBuilder private var actions: some View {
        if CaptureFlow.isSimulator {
            Button("Pair with this Mac") { Task { await flow.fetchFromMac() } }
                .buttonStyle(.primary)
        } else {
            Button {
                flow.showScanner = true
            } label: {
                Label("Scan QR", systemImage: "qrcode.viewfinder")
            }
            .buttonStyle(.primary)
        }
        Details(title: "Paste code") {
            TextField("Pairing code", text: $flow.pasted, axis: .vertical)
                .font(.caption.monospaced())
                .lineLimit(3...6)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(10)
                .background(Theme.surface2, in: RoundedRectangle(cornerRadius: Theme.Radius.control))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.lineStrong))
            Button("Pair") { Task { await flow.pairPasted() } }
                .buttonStyle(.secondary)
                .disabled(flow.pasted.isEmpty)
        }
    }
}

struct PhotoStep: View {
    @Bindable var flow: CaptureFlow
    @State private var picked: PhotosPickerItem?

    var body: some View {
        StepCard(
            number: 2, title: "Photo", value: flow.photoValue, state: flow.photoState,
            reopen: flow.sending ? nil : { if CaptureFlow.isSimulator { flow.retake() } else { flow.openCamera() } },
            reopenHint: "Takes another photo"
        ) {
            VStack(alignment: .leading, spacing: 12) {
                GPSState(status: flow.location.status)
                Group {
                    #if targetEnvironment(simulator)
                        Button {
                            Task { await flow.useSample() }
                        } label: {
                            BusyLabel(busy: flow.photoBusy, title: "Use sample photo", systemImage: "photo")
                        }
                        .buttonStyle(.primary(busy: flow.photoBusy))
                        PhotosPicker(selection: $picked, matching: .images) {
                            Label("Choose photo", systemImage: "photo.on.rectangle")
                        }
                        .buttonStyle(.secondary)
                    #else
                        Button {
                            flow.openCamera()
                        } label: {
                            BusyLabel(busy: flow.photoBusy, title: "Open camera", systemImage: "camera")
                        }
                        .buttonStyle(.primary(busy: flow.photoBusy))
                    #endif
                }
                .disabled(flow.photoBusy)
                if !flow.photoNote.isEmpty { Note(flow.photoNote, tone: flow.photoFailed ? Theme.bad : Theme.muted) }
            }
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

/// A button label that becomes a spinner and "Locating…" while the photo is matched to a fix.
private struct BusyLabel: View {
    let busy: Bool
    let title: String
    let systemImage: String

    var body: some View {
        if busy {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small).tint(Theme.onInk)
                Text("Locating…")
            }
        } else {
            Label(title, systemImage: systemImage)
        }
    }
}

/// The GPS state, shown only when it is not ready: a pill, and for a blocked
/// permission the setting to change.
struct GPSState: View {
    let status: LocationService.Status

    var body: some View {
        switch status {
        case .ok:
            EmptyView()
        case .waiting(let text):
            Pill(text: text, tone: .waiting)
        case .error(let text, let help):
            VStack(alignment: .leading, spacing: 8) {
                Pill(text: text, tone: .bad)
                if let help {
                    Note(help)
                    OpenSettingsButton()
                }
            }
        }
    }
}
