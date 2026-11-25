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

/// Sets this iPhone up on its own: the only thing the person does is confirm
/// with Face ID. A failure offers to try again.
struct PairScreen: View {
    let flow: CaptureFlow

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
            #if DEBUG
                if flow.pairFailed {
                    Text(flow.pairStatus).font(Typeface.sans(13, relativeTo: .footnote)).foregroundStyle(Theme.faint)
                        .fixedSize(horizontal: false, vertical: true)
                }
            #endif
        } actions: {
            actions
        }
        .task { await flow.connect() }
        .onChange(of: flow.key != nil) { _, ready in
            if ready { Task { await flow.connect() } }
        }
    }

    private var title: String {
        guard flow.pairFailed else { return "Setting up this iPhone" }
        return flow.pairing != nil ? "Couldn't get ready" : "Couldn't connect"
    }

    private var subtitle: String {
        guard flow.pairFailed else {
            if CaptureFlow.isSimulator { return "This Mac does the heavy lifting when you publish." }
            return "Confirm with Face ID so this iPhone can sign the photos it takes."
        }
        if CaptureFlow.isSimulator { return "Start Capture on this Mac, then try again." }
        return "Check your connection and try again."
    }

    private var workingTitle: String {
        if flow.key == nil { return "Setting up this iPhone…" }
        if flow.paramsBusy { return "Getting ready…" }
        return "Connecting…"
    }

    @ViewBuilder private var actions: some View {
        if flow.pairing != nil, !flow.isPaired, !flow.paramsBusy {
            Button("Try again") { Task { await flow.retryParams() } }.buttonStyle(.primary)
            Button("Connect again") {
                flow.pairAgain()
                Task { await flow.connect() }
            }
            .buttonStyle(.quiet)
        } else if flow.pairFailed {
            Button("Try again") { Task { await flow.connect() } }.buttonStyle(.primary)
            Color.clear.frame(height: 44)
        } else {
            Color.clear.frame(height: 98)
        }
    }
}
