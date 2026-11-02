import SwiftUI

/// Publishing is a loader: one line for what runs now and a bar for how far
/// along it is. The steps behind it stay out of sight unless one fails.
struct PublishScreen: View {
    let flow: CaptureFlow

    var body: some View {
        VStack(spacing: 0) {
            Spacer()
            if let failure = flow.sendFailure {
                Problem(title: failure.title, message: failure.message)
            } else {
                VStack(spacing: 28) {
                    ApertureLoader(size: 88)
                    VStack(spacing: 8) {
                        Text("Publishing").font(Typeface.title).tight(26).foregroundStyle(Theme.ink)
                        Text(flow.laptopNotice ?? now)
                            .font(Typeface.sans(15, relativeTo: .subheadline)).foregroundStyle(Theme.soft)
                            .contentTransition(.opacity)
                            .animation(Theme.ease, value: now)
                    }
                    .multilineTextAlignment(.center)
                    ProgressLine(value: progress).frame(width: 160)
                }
                .padding(.horizontal, 32)
            }
            Spacer()
            if flow.sendFailure != nil {
                VStack(spacing: 10) {
                    Button("Try again") { flow.publish() }.buttonStyle(.primary)
                    Button("Back") { flow.backToReview() }.buttonStyle(.quiet)
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
            } else {
                Text("Keep the app open.").font(Typeface.sans(13.5, relativeTo: .footnote)).foregroundStyle(Theme.faint)
                    .padding(.bottom, 20)
            }
        }
        .frame(maxWidth: .infinity)
        .background(Theme.paper.ignoresSafeArea())
    }

    private var items: [Timeline.Item] { flow.timeline.flatMap(\.items) }

    /// What runs now, in plain words.
    private var now: String {
        if flow.sendSteps[.prove] == .running { return "Proving you were in the area" }
        if flow.sendSteps[.upload] == .running { return "Sending it to your laptop" }
        let running = items.first { if case .running = $0.state { true } else { false } }
            ?? items.first { if case .pending = $0.state { true } else { false } }
        if let running { return Self.plain[running.row.key] ?? running.row.title }
        return "Almost there"
    }

    private var progress: Double {
        let phone = flow.sendSteps.values.filter { $0 == .done }.count
        let laptop = items.filter {
            switch $0.state {
            case .done, .skipped: true
            default: false
            }
        }.count
        // Before the laptop answers, its steps are not known yet; six is the usual count.
        let total = 2 + max(items.count, 6)
        return Double(phone + laptop) / Double(total)
    }

    /// What each laptop step and check does, in plain words.
    static let plain: [String: String] = [
        "crop": "Cutting out the part you share",
        "location-proof": "Checking the area proof",
        "crop-proof": "Proving the photo wasn't altered",
        "publish": "Adding Content Credentials",
        "manifest": "Checking the Content Credentials",
        "device": "Checking this iPhone's signature",
        "image": "Checking the pixels",
        "location": "Checking the area",
    ]
}

/// A thin bar that fills as the work moves on.
private struct ProgressLine: View {
    let value: Double

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.line)
                Capsule().fill(Theme.ink).frame(width: max(6, geometry.size.width * min(max(value, 0), 1)))
            }
        }
        .frame(height: 4)
        .animation(Theme.ease, value: value)
        .accessibilityElement()
        .accessibilityLabel("Progress")
        .accessibilityValue("\(Int((value * 100).rounded())) percent")
    }
}

/// How it ended: verified, saved on the laptop only, or not published.
struct ResultScreen: View {
    let flow: CaptureFlow

    var body: some View {
        VStack(spacing: 0) {
            switch flow.outcome {
            case .accepted(let claim, let image, let file):
                VerifiedView(image: UIImage(data: image), takenAt: claim.capturedAt, file: file, again: { flow.retake() })
            case .rejected(let failure):
                Ending(again: { flow.retake() }) { Problem(title: failure.title, message: failure.message) }
            case nil:
                Ending(again: { flow.retake() }) {
                    VStack(spacing: 20) {
                        Image(systemName: "tray.and.arrow.down").font(.system(size: 24, weight: .medium)).foregroundStyle(Theme.ink)
                            .frame(width: 64, height: 64).background(Theme.surface, in: Circle())
                            .accessibilityHidden(true)
                        Heading(
                            title: "Saved on your laptop",
                            subtitle: "Publishing is off there. It can be published from the laptop.", center: true)
                    }
                    .padding(.horizontal, 32)
                }
            }
        }
        .background(Theme.paper.ignoresSafeArea())
    }
}

/// A centred message with one way forward.
private struct Ending<Content: View>: View {
    let again: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            Spacer()
            content
            Spacer()
            Button("Take another", action: again).buttonStyle(.primary)
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
        }
    }
}

/// The published photo and the one thing it proves: when, and roughly where.
struct VerifiedView: View {
    let image: UIImage?
    let takenAt: String
    var file: URL?
    var again: (() -> Void)?
    @State private var popped = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 16)
            VStack(spacing: 28) {
                VStack(spacing: 18) {
                    VerifiedMark(size: 72)
                        .scaleEffect(popped ? 1 : 0.4)
                        .opacity(popped ? 1 : 0)
                    Heading(title: "Verified", subtitle: summary, center: true)
                }
                if let image {
                    Image(uiImage: image).resizable().scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
                        .accessibilityLabel("The published photo")
                }
            }
            .padding(.horizontal, 24)
            Spacer(minLength: 16)
            if let file {
                VStack(spacing: 6) {
                    ShareLink(item: file) { Label("Share", systemImage: "square.and.arrow.up") }.buttonStyle(.primary)
                    if let again { Button("Take another", action: again).buttonStyle(.quiet) }
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
            }
        }
        .onAppear {
            if reduceMotion { popped = true } else { withAnimation(.spring(duration: 0.6, bounce: 0.5).delay(0.15)) { popped = true } }
        }
    }

    private var summary: String {
        let today = ISO8601DateFormatter().date(from: takenAt).map(Calendar.current.isDateInToday) ?? false
        return "Taken \(today ? "today at " : "")\(Verdict.when(takenAt)). Your location stays private."
    }
}
