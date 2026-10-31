import SwiftUI

/// A numbered step of the flow. Locked steps are dimmed, done steps fold to their summary.
struct StepCard<Content: View>: View {
    let number: Int
    let title: String
    var summary = ""
    let state: StepState
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                badge
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.headline).foregroundStyle(Theme.ink)
                    if state != .locked, !summary.isEmpty {
                        Text(summary).font(.subheadline).foregroundStyle(Theme.muted)
                    }
                }
                Spacer(minLength: 0)
            }
            if state == .active {
                content
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 14))
        // Content leaving during the collapse animation must not draw outside the card.
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(state == .active ? Theme.accent.opacity(0.35) : Theme.line, lineWidth: 1)
        )
        .opacity(state == .locked ? 0.55 : 1)
        .animation(.easeInOut(duration: 0.2), value: state)
    }

    @ViewBuilder private var badge: some View {
        ZStack {
            Circle().fill(state == .done ? Theme.ok : state == .active ? Theme.accent : Theme.line)
            if state == .done {
                Image(systemName: "checkmark").font(.caption.bold()).foregroundStyle(.white)
            } else {
                Text("\(number)").font(.subheadline.bold())
                    .foregroundStyle(state == .active ? .white : Theme.muted)
            }
        }
        .frame(width: 28, height: 28)
        .accessibilityLabel(state == .done ? "Step \(number), done" : "Step \(number)")
    }
}

struct Hint: View {
    let text: String
    var tone: Color = Theme.muted

    init(_ text: String, tone: Color = Theme.muted) {
        self.text = text
        self.tone = tone
    }

    var body: some View {
        Text(text).font(.footnote).foregroundStyle(tone).fixedSize(horizontal: false, vertical: true)
    }
}

struct Tag: View {
    let text: String
    let isPublic: Bool

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .foregroundStyle(isPublic ? Theme.publicInk : Theme.privateInk)
            .background(isPublic ? Theme.publicSoft : Theme.privateSoft, in: Capsule())
    }
}

struct WideButtonStyle: ButtonStyle {
    var quiet = false
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 13)
            .foregroundStyle(quiet ? Theme.accent : .white)
            .background(quiet ? Theme.accentSoft : Theme.accent, in: RoundedRectangle(cornerRadius: 12))
            .opacity(isEnabled ? (configuration.isPressed ? 0.8 : 1) : 0.45)
    }
}

struct Chip: View {
    let title: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline.weight(.medium))
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .foregroundStyle(selected ? Theme.accent : Theme.ink)
                .background(selected ? Theme.accentSoft : Theme.surface2, in: Capsule())
                .overlay(Capsule().stroke(selected ? Theme.accent.opacity(0.5) : Theme.line))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The mark in front of a timed row: a dot, a spinner, a check or a cross.
struct RowMark: View {
    enum Kind { case pending, running, done, failed, skipped }
    let kind: Kind

    var body: some View {
        Group {
            switch kind {
            case .running: ProgressView().controlSize(.small)
            case .done: Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.ok)
            case .failed: Image(systemName: "xmark.circle.fill").foregroundStyle(Theme.bad)
            case .pending, .skipped: Circle().fill(Theme.line).frame(width: 8, height: 8)
            }
        }
        .frame(width: 20, height: 20)
    }
}

struct TimedRow: View {
    let mark: RowMark.Kind
    let title: String
    var seconds: String?
    var what: String?
    var detail: String?
    var note: String?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            RowMark(kind: mark)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline) {
                    Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(mark == .skipped ? Theme.faint : Theme.ink)
                    Spacer(minLength: 8)
                    if let seconds {
                        Text(seconds).font(.caption.monospacedDigit()).foregroundStyle(Theme.muted)
                    }
                }
                if let what { Text(what).font(.caption).foregroundStyle(Theme.muted) }
                if let detail {
                    Text(detail).font(.caption.monospaced())
                        .foregroundStyle(mark == .failed ? Theme.bad : Theme.ink)
                        .textSelection(.enabled)
                }
                if let note { Text(note).font(.caption2).foregroundStyle(Theme.faint) }
            }
        }
        .padding(.vertical, 6)
    }
}

struct LocalStepsList: View {
    let steps: [LocalStep]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.2)) { context in
            VStack(alignment: .leading, spacing: 0) {
                ForEach(steps) { step in
                    TimedRow(
                        mark: step.state == .running ? .running : step.state == .done ? .done : .failed,
                        title: step.title,
                        seconds: String(format: "%.1f s", step.seconds ?? context.date.timeIntervalSince(step.started)),
                        detail: step.detail)
                }
            }
        }
    }
}

struct LaptopTimeline: View {
    let groups: [Timeline.Group]
    let runningSince: [String: Date]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.5)) { context in
            VStack(alignment: .leading, spacing: 14) {
                ForEach(groups) { group in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(group.title).font(.footnote.weight(.semibold)).foregroundStyle(Theme.muted)
                            .textCase(.uppercase)
                        ForEach(group.items) { item in
                            row(item, now: context.date)
                        }
                    }
                }
            }
        }
    }

    private func row(_ item: Timeline.Item, now: Date) -> TimedRow {
        let what = item.row.what
        switch item.state {
        case .pending:
            return TimedRow(mark: .pending, title: item.row.title, what: what)
        case .skipped:
            return TimedRow(mark: .skipped, title: item.row.title, what: what, note: "Not run")
        case .running(let detail):
            let elapsed = now.timeIntervalSince(runningSince[item.id] ?? now)
            let seconds = item.row.typical.map { String(format: "%.0f s of ~%.0f s", elapsed, $0) }
                ?? String(format: "%.0f s", elapsed)
            return TimedRow(mark: .running, title: item.row.title, seconds: seconds, what: what, detail: detail)
        case .done(let seconds, let detail, let note):
            return TimedRow(
                mark: .done, title: item.row.title, seconds: String(format: "%.1f s", seconds), what: what, detail: detail,
                note: note)
        case .failed(let seconds, let detail, let note):
            return TimedRow(
                mark: .failed, title: item.row.title, seconds: seconds.map { String(format: "%.1f s", $0) }, what: what,
                detail: detail, note: note)
        }
    }
}
