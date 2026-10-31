import Foundation

/// The publish stages and the reader checks as a list, built from the events
/// `provenance serve` reports. The same rows and rules as web/src/timeline.ts.
enum Timeline {
    struct Row: Sendable {
        let key: String
        let title: String
        let what: String
        /// Seconds the step usually takes on a laptop, when it is worth saying.
        var typical: Double? = nil
    }

    enum State: Equatable, Sendable {
        case pending
        case running(detail: String?)
        case done(seconds: Double, detail: String, note: String?)
        case failed(seconds: Double?, detail: String, note: String?)
        case skipped
    }

    struct Item: Identifiable, Sendable {
        let row: Row
        let state: State
        var id: String { row.key }
    }

    struct Group: Identifiable, Sendable {
        let title: String
        let items: [Item]
        var id: String { title }
    }

    static let stages: [Row] = [
        Row(key: "crop", title: "Crop", what: "Cuts the chosen rectangle out of the private original."),
        Row(
            key: "location-proof", title: "Location proof",
            what: "Groth16: the hints sealed in the envelope map to the cell."),
        Row(
            key: "crop-proof", title: "Crop proof",
            what: "HyperVerITAS PST: the published pixels are that rectangle of the fingerprinted original.",
            typical: 15),
        Row(key: "publish", title: "C2PA manifest", what: "Embeds the receipt and both proofs, signed by the editor."),
    ]

    static let checks: [Row] = [
        Row(key: "manifest", title: "C2PA manifest", what: "The file carries a valid C2PA claim with one zkloc assertion."),
        Row(key: "device", title: "Device receipt", what: "A trusted device signed the fingerprint and the envelope together."),
        Row(key: "image", title: "Crop proof", what: "These pixels are the claimed rectangle of the fingerprinted original."),
        Row(key: "location", title: "Location proof", what: "The hints sealed in the signed envelope map to the claimed cell."),
    ]

    /// `halted` marks a step still running as failed, for a job that stopped
    /// with an error between events.
    static func groups(_ events: [Event], halted: String? = nil) -> [Group] {
        var states: [String: State] = [:]
        for event in events {
            switch event {
            case .started(let stage):
                states[stage] = .running(detail: nil)
            case .finished(let stage, let detail, let seconds):
                states[stage] = .done(seconds: seconds, detail: detail, note: nil)
            case .checking(let check, let intent):
                states[check] = .running(detail: intent)
            case .passed(let check, let intent, let outcome, let seconds):
                states[check] = .done(seconds: seconds, detail: outcome, note: intent)
            case .rejected(let check, let intent, let reason, let seconds):
                states[check] = .failed(seconds: seconds, detail: reason, note: intent)
            case .other:
                break
            }
        }
        if let halted {
            for (key, state) in states {
                if case .running = state { states[key] = .failed(seconds: nil, detail: halted, note: nil) }
            }
        }
        let rejected = states.values.contains { if case .failed = $0 { true } else { false } }

        let group = { (title: String, rows: [Row]) -> Group in
            var stopped = false
            let items = rows.map { row -> Item in
                var state = states[row.key] ?? .pending
                if stopped, state == .pending { state = .skipped }
                if case .failed = state { stopped = true }
                if state == .pending, rejected { state = .skipped }
                return Item(row: row, state: state)
            }
            return Group(title: title, items: items)
        }
        return [
            group("On the laptop: prove and sign", stages),
            group("As a reader: verify the file", checks),
        ]
    }
}
