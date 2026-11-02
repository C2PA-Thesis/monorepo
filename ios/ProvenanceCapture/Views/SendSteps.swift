import SwiftUI

struct SendStep: View {
    @Bindable var flow: CaptureFlow
    @State private var open = false

    var body: some View {
        StepCard(
            number: 5, title: "Send", value: flow.sendSummary, state: flow.sendState, open: open,
            reopen: { open.toggle() }, reopenHint: open ? "Hides the steps" : "Shows the steps", expands: true
        ) {
            VStack(alignment: .leading, spacing: 12) {
                if flow.sendState == .active, !flow.sending {
                    VStack(alignment: .leading, spacing: 2) {
                        Note("Published: crop, cell, time, fingerprint, proof")
                        Note("Private: full photo, exact position")
                    }
                    .accessibilityElement(children: .combine)
                    Button("Sign and send") { flow.submit() }
                        .buttonStyle(.primary)
                        .disabled(!flow.canSubmit)
                }
                if !flow.localSteps.isEmpty { LocalStepsList(steps: flow.localSteps) }
            }
        }
        .onChange(of: flow.acceptedID) { open = false }
    }
}

struct LaptopStep: View {
    let flow: CaptureFlow
    @State private var open = false

    var body: some View {
        StepCard(
            number: 6, title: "Publishing", value: flow.laptopValue, state: flow.laptopState, open: open,
            reopen: { open.toggle() }, reopenHint: open ? "Hides the steps" : "Shows the steps", expands: true
        ) {
            VStack(alignment: .leading, spacing: 12) {
                if flow.storedOnly, let id = flow.acceptedID {
                    Text(Self.stored(id)).foregroundStyle(Theme.muted)
                } else {
                    if flow.laptopState == .active, let notice = flow.laptopNotice {
                        Pill(text: notice, tone: .waiting)
                    }
                    LaptopTimeline(groups: flow.timeline, runningSince: flow.runningSince)
                }
            }
            .font(.system(size: 15))
            .foregroundStyle(Theme.ink2)
        }
        .onChange(of: flow.acceptedID) { open = false }
    }

    /// "Stored, not published. Run scripts/publish.sh <id> on the laptop.", the command in mono.
    static func stored(_ id: String) -> AttributedString {
        var command = AttributedString("scripts/publish.sh \(id)")
        command.font = Theme.mono
        return AttributedString("Stored, not published. Run ") + command + AttributedString(" on the laptop.")
    }
}

/// The result in the web page's order: verdict, image, map, facts with
/// "Details", one caveat line, then the actions.
struct ResultCard: View {
    let outcome: Outcome
    let outline: Outline?
    let again: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            switch outcome {
            case .accepted(let claim, let image, let file):
                verdict(ok: true, title: "Verified")
                if let picture = UIImage(data: image) {
                    Image(uiImage: picture).resizable().scaledToFit()
                        .frame(maxWidth: .infinity, maxHeight: 352)
                        .background(Theme.surface2)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
                        .accessibilityLabel("The published crop")
                }
                CellMap(outline: outline?.cell == claim.cell ? outline : nil)
                // The toggle's 32 pt row keeps the rows above and under it at one pitch.
                VStack(alignment: .leading, spacing: 0) {
                    facts {
                        fact("Published", Text(claim.crop.text))
                        fact("Cell", cell(claim))
                        fact("Captured", Text(Verdict.time(claim.capturedAt)))
                    }
                    Details(flush: true) {
                        facts {
                            fact("Key", mono(Verdict.short(claim.device)))
                            fact("Fingerprint", mono(Verdict.short(claim.fingerprint)))
                            fact("Envelope", mono(Verdict.short(claim.envelope)))
                        }
                        Note("Save to Files keeps the file intact. Photos may convert it, and a converted file no longer verifies.")
                    }
                }
                Note(Verdict.caveat)
                HStack(spacing: 8) {
                    ShareLink(item: file) { Text("Share") }.buttonStyle(.primary)
                    Button("Take another", action: again).buttonStyle(.secondary)
                }
            case .rejected(let title, let reason):
                VStack(alignment: .leading, spacing: 6) {
                    verdict(ok: false, title: title)
                    if !reason.isEmpty {
                        Text(reason).font(.system(size: 15)).foregroundStyle(Theme.bad)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                    }
                }
                Button("Take another", action: again).buttonStyle(.secondary)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(CardSurface(elevation: .raised))
    }

    /// A filled mark and the verdict at card-title size.
    private func verdict(ok: Bool, title: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                .font(.system(size: 22)).foregroundStyle(ok ? Theme.ok : Theme.bad)
            Text(title).font(.system(size: 17, weight: .semibold)).foregroundStyle(Theme.ink)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }

    /// The cell, short and in mono, and its area.
    private func cell(_ claim: Claim) -> Text {
        var cell = AttributedString(Verdict.cellShort(claim.cell))
        cell.font = Theme.mono
        let size = Verdict.area(of: claim.cell, outline: outline)
        return Text(size.isEmpty ? cell : cell + AttributedString(" · \(size)"))
    }

    private func mono(_ value: String) -> Text {
        var text = AttributedString(value)
        text.font = Theme.mono
        return Text(text)
    }

    /// Label and value in two columns, as the web page's definition lists. The
    /// label column has one width, so the rows under "Details" line up with the facts.
    private func facts(@ViewBuilder _ rows: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 6) { rows() }
    }

    private func fact(_ label: String, _ value: Text) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text(label).font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.muted)
                .frame(width: Self.labelWidth, alignment: .leading)
            value.font(.system(size: 15).monospacedDigit()).foregroundStyle(Theme.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
        }
        // One pitch whether the value is 15 pt text or a 12 pt mono id.
        .frame(minHeight: 20)
        .accessibilityElement(children: .combine)
    }

    /// Fits "Fingerprint", the longest label.
    private static let labelWidth: CGFloat = 84
}
