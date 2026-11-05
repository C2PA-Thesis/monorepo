import SwiftUI

/// One first-run page: a drawing on top, then the words and the actions at
/// the bottom, where the thumb is.
private struct OnboardingPage<Art: View, Status: View, Actions: View>: View {
    let title: String
    let subtitle: String
    @ViewBuilder var art: Art
    @ViewBuilder var status: Status
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            art.frame(maxWidth: .infinity, maxHeight: .infinity)
            VStack(alignment: .leading, spacing: 10) {
                Text(title).font(Typeface.sans(30, .bold, relativeTo: .largeTitle)).tight(30).foregroundStyle(Theme.ink)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                Text(subtitle).font(Typeface.sans(17, relativeTo: .body)).foregroundStyle(Theme.soft).lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
                status.padding(.top, 8)
            }
            .id(title)
            .transition(.opacity)
            VStack(spacing: 4) { actions }
                .padding(.top, 28)
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 8)
        .background(Theme.paper.ignoresSafeArea())
    }
}

extension OnboardingPage where Status == EmptyView {
    init(title: String, subtitle: String, @ViewBuilder art: () -> Art, @ViewBuilder actions: () -> Actions) {
        self.init(title: title, subtitle: subtitle, art: art, status: { EmptyView() }, actions: actions)
    }
}

/// Two pages on what the app is for, before it asks for anything.
struct WelcomeScreen: View {
    let flow: CaptureFlow
    @State private var page = 0

    var body: some View {
        Group {
            if page == 0 {
                OnboardingPage(
                    title: "Take photos anyone can verify",
                    subtitle: "Each photo carries proof of when and where it was taken."
                ) {
                    VerifiedPhotoArt()
                } actions: {
                    Button("Next") { withAnimation(Theme.ease) { page = 1 } }.buttonStyle(.primary)
                    Button("Skip") { finish() }.buttonStyle(.quiet)
                }
            } else {
                OnboardingPage(
                    title: "Your location stays private",
                    subtitle: "People see the neighbourhood, never your exact spot."
                ) {
                    PrivateAreaArt()
                } actions: {
                    Button("Get started") { finish() }.buttonStyle(.primary)
                    Color.clear.frame(height: 44)
                }
            }
        }
        .transition(.opacity)
    }

    private func finish() { withAnimation(Theme.ease) { flow.finishWelcome() } }
}

struct LocationScreen: View {
    let flow: CaptureFlow

    private var denied: Bool { flow.location.access == .denied }

    var body: some View {
        OnboardingPage(
            title: denied ? "Location is off" : "Allow location",
            subtitle: denied
                ? "Turn it on in Settings, with Precise Location."
                : "So each photo can prove where it was taken. Your location stays private."
        ) {
            LocationArt(off: denied)
        } actions: {
            if denied {
                OpenSettingsButton(style: PillButtonStyle(kind: .primary))
            } else {
                Button("Allow location") { flow.allowLocation() }.buttonStyle(.primary)
            }
            Color.clear.frame(height: 44)
        }
    }
}

struct CameraAccessScreen: View {
    let flow: CaptureFlow

    var body: some View {
        OnboardingPage(
            title: "Allow camera",
            subtitle: "Photos you take here are certified the moment you shoot."
        ) {
            ShutterArt()
        } actions: {
            Button("Allow camera") { Task { await flow.allowCamera() } }.buttonStyle(.primary)
            Color.clear.frame(height: 44)
        }
    }
}

struct PairScreen: View {
    @Bindable var flow: CaptureFlow
    @State private var typing = false

    var body: some View {
        OnboardingPage(title: title, subtitle: subtitle) {
            ConnectArt(active: flow.pairWorking)
        } status: {
            if flow.pairWorking {
                HStack(spacing: 10) {
                    Spinner().frame(width: 16, height: 16)
                    Text(workingTitle).font(Typeface.sans(15, .medium, relativeTo: .subheadline)).foregroundStyle(Theme.soft)
                }
                .accessibilityElement(children: .combine)
            }
        } actions: {
            actions
        }
        .sheet(isPresented: $typing) { CodeSheet(flow: flow, done: { typing = false }) }
        .task {
            // The Simulator shares the Mac's network, so it connects without a code.
            if CaptureFlow.isSimulator, flow.pairing == nil, !flow.pairFailed, flow.key != nil { await flow.fetchFromMac() }
        }
        .onChange(of: flow.key != nil) { _, ready in
            if ready, CaptureFlow.isSimulator, flow.pairing == nil, !flow.pairFailed { Task { await flow.fetchFromMac() } }
        }
    }

    private var title: String {
        guard flow.pairFailed else { return "Connect your laptop" }
        if flow.pairNeedsSettings { return "Capture can't see your laptop" }
        if flow.pairing != nil { return "Couldn't get ready" }
        return "Couldn't connect"
    }

    private var subtitle: String {
        guard flow.pairFailed else {
            return CaptureFlow.isSimulator
                ? "This Mac does the heavy lifting when you publish."
                : "It does the heavy lifting when you publish. Open Capture on it and scan the code."
        }
        if flow.pairNeedsSettings { return "Turn on Local Network for Capture in Settings." }
        if flow.pairing != nil { return "Keep your laptop on and try again." }
        return CaptureFlow.isSimulator
            ? "Start Capture on this Mac, then try again."
            : "Check your laptop is on the same Wi-Fi with Capture open."
    }

    private var workingTitle: String {
        if flow.key == nil { return "Setting up this iPhone…" }
        if flow.paramsBusy { return "Getting ready…" }
        return "Connecting…"
    }

    @ViewBuilder private var actions: some View {
        if flow.pairNeedsSettings {
            OpenSettingsButton(style: PillButtonStyle(kind: .primary))
            Button("Try again") { flow.pairAgain() }.buttonStyle(.quiet)
        } else if flow.pairing != nil, !flow.isPaired, !flow.paramsBusy {
            Button("Try again") { Task { await flow.retryParams() } }.buttonStyle(.primary)
            Button("Connect again") { flow.pairAgain() }.buttonStyle(.quiet)
        } else if !flow.pairWorking {
            if CaptureFlow.isSimulator {
                Button(flow.pairFailed ? "Try again" : "Connect to this Mac") { Task { await flow.fetchFromMac() } }
                    .buttonStyle(.primary)
                Color.clear.frame(height: 44)
            } else {
                Button {
                    flow.showScanner = true
                } label: {
                    Label(flow.pairFailed ? "Scan again" : "Scan the code", systemImage: "qrcode.viewfinder")
                }
                .buttonStyle(.primary)
                Button("Enter the code instead") { typing = true }.buttonStyle(.quiet)
            }
        } else {
            Color.clear.frame(height: 98)
        }
    }
}

/// Pasting the pairing code, for when the camera can't read it.
private struct CodeSheet: View {
    @Bindable var flow: CaptureFlow
    let done: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Heading(title: "Enter the code", subtitle: "Copy the pairing code from the Capture page on your laptop and paste it here.")
            TextField("Pairing code", text: $flow.pasted, axis: .vertical)
                .font(Typeface.mono(13))
                .lineLimit(3...6)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(14)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.sm))
                .overlay(RoundedRectangle(cornerRadius: Theme.Radius.sm).strokeBorder(Theme.line2))
            Spacer(minLength: 0)
            Button("Connect") {
                done()
                Task { await flow.pairPasted() }
            }
            .buttonStyle(.primary)
            .disabled(flow.pasted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(24)
        .background(Theme.paper.ignoresSafeArea())
        .presentationDetents([.medium])
        .presentationCornerRadius(Theme.Radius.lg)
    }
}

