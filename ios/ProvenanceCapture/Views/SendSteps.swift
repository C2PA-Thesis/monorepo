import SwiftUI

struct SendStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        StepCard(number: 5, title: "Sign and send", summary: flow.sendSummary, state: flow.sendState) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    column(Tag(text: "Published", isPublic: true), flow.published)
                    column(
                        Tag(text: "Never published", isPublic: false),
                        ["The rest of the frame", "Your exact coordinate", "The random salt that seals it"])
                }
                Hint(
                    "The laptop receives the whole frame, because it builds the crop proof. The coordinate and the salt never leave this phone: the location proof is made here."
                )
                Button("Sign and send to the laptop") { flow.submit() }
                    .buttonStyle(WideButtonStyle())
                    .disabled(!flow.canSubmit)
                if !flow.localSteps.isEmpty { LocalStepsList(steps: flow.localSteps) }
            }
        }
    }

    private func column(_ tag: Tag, _ items: [String]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            tag
            ForEach(items, id: \.self) { item in
                Text(item).font(.caption).foregroundStyle(Theme.ink).fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct LaptopStep: View {
    let flow: CaptureFlow

    var body: some View {
        StepCard(
            number: 6, title: "The laptop publishes and verifies", summary: flow.laptopSummary,
            state: flow.laptopState == .done ? .active : flow.laptopState
        ) {
            if flow.storedOnly, let id = flow.acceptedID {
                Text("The laptop stored the capture without publishing it. Publish it there with ")
                    + Text("scripts/publish.sh \(id)").font(.footnote.monospaced()) + Text(".")
            } else {
                LaptopTimeline(groups: flow.timeline, runningSince: flow.runningSince)
            }
        }
    }
}

struct ResultCard: View {
    let outcome: Outcome
    let api: LaptopAPI?
    let again: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            switch outcome {
            case .accepted(let claim, let image, let file):
                verdict(ok: true, title: "Verified") {
                    Text("These pixels are the \(claim.crop.text) of an original that a trusted device signed together with an envelope whose sealed hints map to cell ")
                        + Text(claim.cell).font(.subheadline.monospaced()) + Text(".")
                }
                if let picture = UIImage(data: image) {
                    Image(uiImage: picture).resizable().scaledToFit()
                        .padding(8)
                        .background(checker, in: RoundedRectangle(cornerRadius: 8))
                        .accessibilityLabel("The published crop")
                }
                facts(claim)
                ResultMap(cell: claim.cell, api: api)
                ShareLink(item: file) { Label("Share the signed file", systemImage: "square.and.arrow.up") }
                    .buttonStyle(WideButtonStyle())
                Hint("Use Save to Files to keep the signed PNG byte for byte. Photos may convert the file, and a converted file no longer verifies.")
                Button("Take another", action: again).buttonStyle(WideButtonStyle(quiet: true))
                Text(Verdict.caveat).font(.caption).foregroundStyle(Theme.muted)
            case .rejected(let title, let reason):
                verdict(ok: false, title: title) { Text(reason) }
                Button("Take another", action: again).buttonStyle(WideButtonStyle(quiet: true))
            }
        }
        .padding(16)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Theme.line))
    }

    private var checker: Color { Theme.surface2 }

    private func verdict(ok: Bool, title: String, @ViewBuilder text: () -> Text) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: ok ? "checkmark.seal.fill" : "xmark.octagon.fill")
                .font(.title).foregroundStyle(ok ? Theme.ok : Theme.bad)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.title3.bold())
                text().font(.subheadline).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ok ? Theme.okSoft : Theme.badSoft, in: RoundedRectangle(cornerRadius: 10))
    }

    private func facts(_ claim: Claim) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 6) {
            fact("Published", claim.crop.text)
            fact("Cell", claim.cell, mono: true)
            fact("Captured", "\(Verdict.time(claim.capturedAt)) (device clock)")
            fact("Device", Verdict.short(claim.device), mono: true)
            fact("Fingerprint", Verdict.short(claim.fingerprint), mono: true)
            fact("Envelope", Verdict.short(claim.envelope), mono: true)
        }
        .font(.subheadline)
    }

    private func fact(_ label: String, _ value: String, mono: Bool = false) -> some View {
        GridRow {
            Text(label).foregroundStyle(Theme.muted)
            Text(value).font(mono ? .subheadline.monospaced() : .subheadline).textSelection(.enabled)
        }
    }
}

/// The published cell alone, as a reader sees it.
struct ResultMap: View {
    let cell: String
    let api: LaptopAPI?
    @State private var outline: Outline?

    var body: some View {
        CellMap(outline: outline)
            .task(id: cell) { outline = try? await api?.outline(cell) }
    }
}
