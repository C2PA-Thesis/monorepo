import SwiftUI

/// A numbered step of the flow. The active card holds the step; a done card
/// folds to its title and value, and tapping it runs `reopen`. A locked step is
/// a bare row, as on the web page. Inside a `StepStack` group the card draws no
/// surface of its own.
struct StepCard<Content: View>: View {
    let number: Int
    let title: String
    var value = StepValue()
    let state: StepState
    /// A done card shows its body again.
    var open = false
    var reopen: (() -> Void)?
    var reopenHint = ""
    /// `reopen` shows the body again, so the done header gets a chevron.
    var expands = false
    /// Only the card that holds the primary action is raised.
    var raised: Bool?
    @ViewBuilder var content: Content
    @Environment(\.inStepGroup) private var grouped

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if state == .done, let reopen {
                Button {
                    withAnimation(.easeOut(duration: 0.18)) { reopen() }
                } label: { header }
                    .buttonStyle(.plain)
                    .accessibilityHint(reopenHint)
            } else {
                header
            }
            if state == .active || (state == .done && open) {
                content.transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, state == .locked ? 10 : grouped ? 12 : 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(CardSurface(on: !grouped && state != .locked, elevation: elevation))
        .overlay(alignment: .top) {
            if grouped, number > 1 { Rectangle().fill(Theme.line).frame(height: 1) }
        }
        .animation(.easeOut(duration: 0.18), value: state)
        .animation(.easeOut(duration: 0.18), value: open)
    }

    private var elevation: Elevation {
        switch state {
        case .active: (raised ?? true) ? .raised : .card
        case .done: .card
        case .locked: .flat
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            StepMark(number: number, state: state)
            Text(title).font(.system(size: 17, weight: .semibold))
                .foregroundStyle(state == .locked ? Theme.faint : Theme.ink)
            Spacer(minLength: 8)
            if state == .done, !value.isEmpty {
                value.text.font(.system(size: 13, weight: .medium).monospacedDigit()).foregroundStyle(Theme.muted)
                    .lineLimit(1).truncationMode(.middle)
            }
            if state == .done, expands, reopen != nil {
                Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Theme.faint).rotationEffect(.degrees(open ? 180 : 0))
                    .accessibilityHidden(true)
            }
        }
        .frame(minHeight: 22)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
    }

    private var accessibilityText: String {
        switch state {
        case .done: "Step \(number), \(title), done" + (value.isEmpty ? "" : ", \(value.string)")
        case .active: "Step \(number), \(title)"
        case .locked: "Step \(number), \(title), not yet"
        }
    }
}

/// A done step's value: plain text, with identifiers set in 12 pt mono as the
/// web page sets them ("Paired · 1e94…a847").
struct StepValue: Equatable, ExpressibleByStringLiteral {
    struct Part: Equatable {
        let text: String
        let mono: Bool
    }

    var parts: [Part] = []

    init(_ text: String = "") {
        parts = text.isEmpty ? [] : [Part(text: text, mono: false)]
    }

    init(stringLiteral text: String) { self.init(text) }

    static func mono(_ text: String) -> StepValue {
        var value = StepValue()
        value.parts = [Part(text: text, mono: true)]
        return value
    }

    static func + (left: StepValue, right: StepValue) -> StepValue {
        var value = left
        value.parts += right.parts
        return value
    }

    var isEmpty: Bool { parts.allSatisfy(\.text.isEmpty) }
    var string: String { parts.map(\.text).joined() }

    var text: Text {
        var joined = AttributedString()
        for part in parts {
            var run = AttributedString(part.text)
            if part.mono { run.font = Theme.mono }
            joined += run
        }
        return Text(joined)
    }
}

/// The card surface: fill, border and one of the two shadows.
struct CardSurface: ViewModifier {
    var on = true
    let elevation: Elevation

    func body(content: Content) -> some View {
        if on {
            content
                .background(Theme.surface, in: card)
                // Content leaving during the collapse animation must not draw outside the card.
                .clipShape(card)
                .overlay(card.strokeBorder(Theme.border(elevation)))
                .elevation(elevation)
        } else {
            content
        }
    }

    private var card: RoundedRectangle { RoundedRectangle(cornerRadius: Theme.Radius.card) }
}

private struct InStepGroupKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    var inStepGroup: Bool {
        get { self[InStepGroupKey.self] }
        set { self[InStepGroupKey.self] = newValue }
    }
}

/// The step cards. Once every step is done they become rows of one card,
/// split by hairlines, so the result is the only large object on screen.
struct StepStack<Content: View>: View {
    let grouped: Bool
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: grouped ? 0 : 12) { content }
            .environment(\.inStepGroup, grouped)
            .background(grouped ? Theme.surface : .clear, in: card)
            .overlay(card.strokeBorder(grouped ? Theme.line : .clear))
            .elevation(grouped ? .card : .flat)
            .animation(.easeOut(duration: 0.18), value: grouped)
    }

    private var card: RoundedRectangle { RoundedRectangle(cornerRadius: Theme.Radius.card) }
}

private struct StepMark: View {
    let number: Int
    let state: StepState

    var body: some View {
        ZStack {
            switch state {
            case .done:
                // Green is kept for verdicts: a finished step is an ink circle with a check.
                Circle().fill(Theme.ink)
                Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(Theme.onInk)
            case .active:
                Circle().fill(Theme.ink)
                Text("\(number)").font(.system(size: 13, weight: .semibold)).foregroundStyle(Theme.onInk)
            case .locked:
                Circle().strokeBorder(Theme.lineStrong, lineWidth: 1.5)
                Text("\(number)").font(.system(size: 13, weight: .semibold)).foregroundStyle(Theme.faint)
            }
        }
        .frame(width: 22, height: 22)
        .accessibilityHidden(true)
    }
}

/// One short muted line, or a failure in bad color.
struct Note: View {
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

/// A state that needs attention: waiting, blocked or a warning. Waiting is a
/// warn pill with a pulsing dot, as on the web page.
struct Pill: View {
    enum Tone { case waiting, bad, warn }
    let text: String
    let tone: Tone
    @State private var dim = false

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(color).frame(width: 6, height: 6)
                .opacity(tone == .waiting && dim ? 0.25 : 1)
                .animation(tone == .waiting ? .easeInOut(duration: 0.6).repeatForever() : nil, value: dim)
                .onAppear { if tone == .waiting { dim = true } }
            Text(text).font(.system(size: 13, weight: .medium)).foregroundStyle(color)
        }
        .padding(.horizontal, 10)
        .frame(height: 24)
        .background(background, in: Capsule())
    }

    private var color: Color { tone == .bad ? Theme.bad : Theme.warn }
    private var background: Color { tone == .bad ? Theme.badSoft : Theme.warnSoft }
}

/// Ink fill, 44 pt, 15/600. A disabled button fades as on the web page.
struct PrimaryButtonStyle: ButtonStyle {
    /// The action runs: the button keeps its full ink fill while it is disabled.
    var busy = false
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 15, weight: .semibold))
            .frame(maxWidth: .infinity, minHeight: 44)
            .foregroundStyle(Theme.onInk)
            .background(Theme.ink, in: RoundedRectangle(cornerRadius: Theme.Radius.control))
            .opacity(isEnabled || busy ? (configuration.isPressed ? 0.88 : 1) : 0.35)
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
    }
}

/// Surface, a strong hairline and ink text.
struct SecondaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 15, weight: .semibold))
            .frame(maxWidth: .infinity, minHeight: 44)
            .foregroundStyle(Theme.ink)
            .background(configuration.isPressed ? Theme.surface2 : Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.control))
            .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.lineStrong))
            .opacity(isEnabled ? 1 : 0.35)
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
    }
}

extension ButtonStyle where Self == PrimaryButtonStyle {
    static var primary: PrimaryButtonStyle { PrimaryButtonStyle() }
    static func primary(busy: Bool) -> PrimaryButtonStyle { PrimaryButtonStyle(busy: busy) }
}

extension ButtonStyle where Self == SecondaryButtonStyle {
    static var secondary: SecondaryButtonStyle { SecondaryButtonStyle() }
}

/// Opens this app's page in Settings.
struct OpenSettingsButton: View {
    var body: some View {
        if let url = URL(string: UIApplication.openSettingsURLString) {
            Link("Open Settings", destination: url).buttonStyle(.secondary)
        }
    }
}

struct Chip: View {
    let title: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 13, weight: .medium))
                .padding(.horizontal, 12)
                .frame(minHeight: 32)
                .foregroundStyle(selected ? Theme.onInk : Theme.ink)
                .background(selected ? Theme.ink : Theme.surface2, in: Capsule())
                // A 44 pt target around the 32 pt chip.
                .padding(.vertical, 6)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// "Details": a quiet disclosure for what is hidden by default.
struct Details<Content: View>: View {
    var title = "Details"
    /// The body continues the rows above the toggle at their own spacing, with no gap of its own.
    var flush = false
    @ViewBuilder var content: Content
    @State private var open = false

    var body: some View {
        DisclosureGroup(isExpanded: $open) {
            VStack(alignment: .leading, spacing: 8) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, flush ? 0 : 4)
        } label: {
            Text(title)
        }
        .disclosureGroupStyle(QuietDisclosure(spacing: flush ? 0 : 8))
    }
}

/// The title and a small chevron in muted, the body under it.
private struct QuietDisclosure: DisclosureGroupStyle {
    let spacing: CGFloat

    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: spacing) {
            Button {
                withAnimation(.easeOut(duration: 0.15)) { configuration.isExpanded.toggle() }
            } label: {
                HStack(spacing: 6) {
                    configuration.label.font(.footnote.weight(.medium))
                    Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold))
                        .rotationEffect(.degrees(configuration.isExpanded ? 180 : 0))
                    Spacer(minLength: 0)
                }
                .foregroundStyle(Theme.muted)
                .frame(minHeight: 32)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityValue(configuration.isExpanded ? "expanded" : "collapsed")
            if configuration.isExpanded { configuration.content }
        }
    }
}

/// The mark in front of a status row, as web/src/style.css draws it: a faint
/// dot, a spinner, a plain check in ink, or a cross on a bad circle.
struct RowMark: View {
    enum Kind { case pending, running, done, failed, skipped }
    let kind: Kind

    var body: some View {
        Group {
            switch kind {
            case .running: ProgressView().controlSize(.small).tint(Theme.ink).scaleEffect(0.8)
            case .done: Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Theme.ink2)
            case .failed: Image(systemName: "xmark.circle.fill").font(.system(size: 18)).foregroundStyle(Theme.bad)
            case .pending, .skipped: Circle().fill(Theme.faint).frame(width: 6, height: 6)
            }
        }
        .frame(width: RowMark.width, height: RowMark.width)
        .accessibilityHidden(true)
    }

    static let width: CGFloat = 18
    /// Where a row's title starts: the mark and the gap after it.
    static let indent: CGFloat = width + 10
}

/// Mark, title and time on one line. What the step establishes and what it
/// found sit behind a tap; only a failure reason shows inline.
struct StatusRow: View {
    let mark: RowMark.Kind
    let title: String
    var time: String?
    var details: [(text: String, mono: Bool)] = []
    var failure: String?
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if details.isEmpty {
                line.accessibilityElement(children: .combine).accessibilityLabel(accessibilityText)
            } else {
                Button { open.toggle() } label: { line }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(accessibilityText)
                    .accessibilityValue(open ? "expanded" : "collapsed")
            }
            if let failure {
                Text(failure).font(.system(size: 13)).foregroundStyle(Theme.bad).padding(.leading, RowMark.indent)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .padding(.bottom, 6)
            }
            if open {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(details.enumerated()), id: \.offset) { _, line in
                        Text(line.text)
                            .font(line.mono ? Theme.mono : .system(size: 13))
                            .foregroundStyle(Theme.ink2)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                    }
                }
                .padding(.vertical, 10)
                .padding(.horizontal, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.surface2, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
                .padding(.leading, RowMark.indent)
                .padding(.bottom, 10)
            }
        }
        .animation(.easeOut(duration: 0.15), value: open)
    }

    private var line: some View {
        RowLine(mark: mark, title: title, time: time, chevron: details.isEmpty ? nil : open)
    }

    private var accessibilityText: String {
        [title, mark.spoken, time].compactMap { $0 }.joined(separator: ", ")
    }
}

extension RowMark.Kind {
    var spoken: String {
        switch self {
        case .pending: "waiting"
        case .running: "running"
        case .done: "done"
        case .failed: "failed"
        case .skipped: "not run"
        }
    }
}

/// One 36 pt line of a status list. `chevron` is the open state of a row that
/// opens; a row that does not keeps a clear one, so times line up.
private struct RowLine: View {
    let mark: RowMark.Kind
    let title: String
    var time: String?
    var chevron: Bool?

    var body: some View {
        HStack(spacing: 10) {
            RowMark(kind: mark)
            Text(title).font(.system(size: 15))
                .foregroundStyle(mark == .skipped || mark == .pending ? Theme.faint : Theme.ink)
            Spacer(minLength: 8)
            if let time {
                Text(time).font(.system(size: 13).monospacedDigit()).foregroundStyle(chevron == true ? Theme.ink : Theme.muted)
            }
            Image(systemName: "chevron.down").font(.system(size: 9, weight: .semibold))
                .foregroundStyle(chevron == nil ? .clear : Theme.faint).rotationEffect(.degrees(chevron == true ? 180 : 0))
                .accessibilityHidden(true)
        }
        .frame(minHeight: 36)
        .contentShape(Rectangle())
    }
}

struct LocalStepsList: View {
    let steps: [LocalStep]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.2)) { context in
            VStack(alignment: .leading, spacing: 0) {
                ForEach(steps) { step in
                    StatusRow(
                        mark: step.state == .running ? .running : step.state == .done ? .done : .failed,
                        title: step.title,
                        time: Verdict.rowTime(step.seconds ?? context.date.timeIntervalSince(step.started)),
                        details: step.state == .failed ? [] : step.detail.map { [($0, true)] } ?? [],
                        failure: step.state == .failed ? step.detail : nil)
                }
            }
        }
    }
}

/// The laptop's two groups, as web/src/timeline.ts draws them: a group that
/// fully passed folds into one row with its total time; a running or stopped
/// group shows its steps under its row; a group not reached is one faint row.
struct LaptopTimeline: View {
    let groups: [Timeline.Group]
    let runningSince: [String: Date]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.5)) { context in
            VStack(alignment: .leading, spacing: 16) {
                ForEach(groups) { group in
                    GroupRows(group: group, runningSince: runningSince, now: context.date)
                }
            }
        }
    }

    /// What a step found, as a value, the same as web/src/timeline.ts outcome():
    /// "kept 512x512 at (0, 0)" reads "512×512 at (0, 0)" and "proof accepted"
    /// reads "Accepted".
    nonisolated static func outcome(_ detail: String) -> String {
        var value = detail.trimmingCharacters(in: .whitespaces)
            .replacing(/^(?i:kept|proof) (?=\S)/, with: "")
            .replacing(/(\d)x(\d)/) { "\($0.1)×\($0.2)" }
        if value.hasSuffix(".") { value.removeLast() }
        guard let first = value.first else { return value }
        return first.uppercased() + value.dropFirst()
    }
}

private struct GroupRows: View {
    let group: Timeline.Group
    let runningSince: [String: Date]
    let now: Date
    @State private var open = false

    var body: some View {
        let states = group.items.map(\.state)
        if states.allSatisfy({ $0 == .pending || $0 == .skipped }) {
            StatusRow(mark: .pending, title: group.title)
        } else if states.allSatisfy({ if case .done = $0 { true } else { false } }) {
            let total = group.seconds
            VStack(alignment: .leading, spacing: 0) {
                Button { open.toggle() } label: {
                    RowLine(mark: .done, title: group.title, time: Verdict.seconds(total), chevron: open)
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .combine)
                .accessibilityLabel("\(group.title), done, \(Verdict.seconds(total))")
                .accessibilityValue(open ? "expanded" : "collapsed")
                if open { items }
            }
            .animation(.easeOut(duration: 0.15), value: open)
        } else {
            let failed = states.contains { if case .failed = $0 { true } else { false } }
            VStack(alignment: .leading, spacing: 0) {
                StatusRow(mark: failed ? .failed : .running, title: group.title, time: failed ? nil : elapsed)
                    .accessibilityAddTraits(.isHeader)
                items
            }
        }
    }

    private var items: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(group.items) { item in row(item) }
        }
        .padding(.leading, RowMark.indent)
        .padding(.bottom, 4)
    }

    /// Since the group's first step was seen running.
    private var elapsed: String {
        let since = group.items.compactMap { runningSince[$0.id] }.min() ?? now
        return String(format: "%.0f s", now.timeIntervalSince(since))
    }

    private func row(_ item: Timeline.Item) -> StatusRow {
        let row = item.row
        switch item.state {
        case .pending:
            return StatusRow(mark: .pending, title: row.title)
        case .skipped:
            return StatusRow(mark: .skipped, title: row.title)
        case .running:
            let elapsed = now.timeIntervalSince(runningSince[item.id] ?? now)
            let time = row.typical.map { String(format: "%.0f / ~%.0f s", elapsed, $0) } ?? String(format: "%.0f s", elapsed)
            return StatusRow(mark: .running, title: row.title, time: time)
        case .done(let seconds, let detail, _):
            return StatusRow(
                mark: .done, title: row.title, time: Verdict.rowTime(seconds), details: lines(row, detail))
        case .failed(let seconds, let detail, _):
            return StatusRow(
                mark: .failed, title: row.title, time: seconds.flatMap(Verdict.rowTime),
                details: lines(row, nil), failure: detail)
        }
    }

    private func lines(_ row: Timeline.Row, _ detail: String?) -> [(text: String, mono: Bool)] {
        var lines = [(text: row.what, mono: false)]
        if let detail, !detail.isEmpty { lines.append((LaptopTimeline.outcome(detail), true)) }
        return lines
    }
}

/// Label left, value right, the slider under them, as the web page's Size control.
struct LabeledSlider: View {
    let label: String
    let value: String
    @Binding var position: Double
    let range: ClosedRange<Double>
    var step: Double?
    /// The slider's VoiceOver name.
    let spoken: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(label).font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.muted)
                Spacer()
                Text(value).font(.system(size: 15).monospacedDigit()).foregroundStyle(Theme.ink)
            }
            .accessibilityHidden(true)
            BareSlider(position: $position, range: range, step: step, spoken: spoken, value: value)
        }
    }
}

/// An ink slider with no label row, as the web page's range inputs.
struct BareSlider: View {
    @Binding var position: Double
    let range: ClosedRange<Double>
    var step: Double?
    let spoken: String
    let value: String

    var body: some View {
        Group {
            if let step {
                Slider(value: $position, in: range, step: step)
            } else {
                Slider(value: $position, in: range)
            }
        }
        .tint(Theme.ink)
        .accessibilityLabel(spoken)
        .accessibilityValue(value)
    }
}
