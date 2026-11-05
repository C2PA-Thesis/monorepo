import Foundation

/// How sizes, identifiers and times read on screen, as web/src/dom.ts and
/// web/src/verdict.ts write them.
enum Verdict {
    /// The resolutions the server accepts (crates/provenance, 4..=9).
    static let resolutions = 4...9
    static let defaultResolution = 7

    /// Average hexagon area in km² per resolution, from h3geo.org, "Tables of
    /// Cell Statistics Across Resolutions". A given cell can differ by a factor of two.
    static let averageKm2: [Double] = [
        4357449.416, 609788.442, 86801.78, 12393.435, 1770.348, 252.904, 36.129, 5.161, 0.737, 0.105, 0.015,
        0.00215, 0.000307, 0.0000439, 0.00000627, 0.000000895,
    ]

    static let scale = [
        "a continent", "a subcontinent", "a large country", "a province", "a metropolitan area", "a city",
        "a district", "a neighbourhood", "a few blocks", "a city block", "a large building", "a building",
        "a house", "a room", "a desk", "a floor tile",
    ]

    /// The one honest limit, said once on a result. The README has the long version.
    static let caveat = "The cell holds if the device computed the sealed hints from its position."

    /// The average size of a cell at `resolution`.
    static func resolutionText(_ resolution: Int) -> String {
        "~\(area(averageKm2[resolution])) · \(scale[resolution])"
    }

    /// An H3 index without its unused trailing digits, as the cell is named on screen.
    static func cellShort(_ cell: String) -> String {
        var trimmed = Substring(cell)
        while trimmed.last == "f" { trimmed = trimmed.dropLast() }
        return trimmed.count < cell.count ? trimmed + "…" : cell
    }

    /// The resolution an H3 index encodes in bits 52 to 55.
    static func resolution(of cell: String) -> Int? {
        UInt64(cell, radix: 16).map { Int($0 >> 52 & 0xf) }
    }

    /// The cell's real area once its outline is known, the average for its resolution before.
    static func area(of cell: String, outline: Outline?) -> String {
        if let outline, outline.cell == cell { return area(outline.areaKm2) }
        guard let resolution = resolution(of: cell), averageKm2.indices.contains(resolution) else { return "" }
        return "~\(area(averageKm2[resolution]))"
    }

    static func area(_ km2: Double) -> String {
        if km2 >= 100 { return "\(grouped(km2.rounded())) km²" }
        if km2 >= 1 { return String(format: "%.1f km²", km2) }
        let m2 = km2 * 1e6
        return m2 >= 10_000 ? String(format: "%.1f ha", m2 / 10_000) : "\(grouped(m2.rounded())) m²"
    }

    /// First and last four hex digits, as the CLI shows identifiers.
    static func short(_ hex: String) -> String {
        let prefixed = hex.hasPrefix("0x")
        let digits = prefixed ? String(hex.dropFirst(2)) : hex
        guard digits.count > 10 else { return hex }
        return (prefixed ? "0x" : "") + digits.prefix(4) + "…" + digits.suffix(4)
    }

    /// A step time. A step under 0.05 s reads "<0.1 s", not "0.0 s".
    static func seconds(_ value: Double) -> String {
        value < 0.05 ? "<0.1 s" : String(format: "%.1f s", value)
    }

    /// A row's time. Steps under 0.05 s show none: a column of "<0.1 s" is noise.
    static func rowTime(_ value: Double) -> String? {
        value >= 0.05 ? seconds(value) : nil
    }

    static func time(_ rfc3339: String) -> String {
        guard let date = ISO8601DateFormatter().date(from: rfc3339) else { return rfc3339 }
        return date.formatted(date: .abbreviated, time: .standard)
    }

    /// The time of day today, the date and time otherwise.
    static func when(_ rfc3339: String) -> String {
        guard let date = ISO8601DateFormatter().date(from: rfc3339) else { return rfc3339 }
        return Calendar.current.isDateInToday(date)
            ? date.formatted(date: .omitted, time: .shortened) : date.formatted(date: .abbreviated, time: .shortened)
    }

    private static func grouped(_ value: Double) -> String {
        let formatter = NumberFormatter()
        formatter.locale = Locale(identifier: "en_US")
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 0
        return formatter.string(from: NSNumber(value: value)) ?? String(Int(value))
    }
}
