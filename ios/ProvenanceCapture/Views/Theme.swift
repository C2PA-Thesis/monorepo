import SwiftUI
import UIKit

/// The tokens of the design system, shared with web/src/style.css. Ink on paper;
/// color only for success, failure and warning.
enum Theme {
    static let bg = Color(light: 0xF6F6F4, dark: 0x0A0A0B)
    static let surface = Color(light: 0xFFFFFF, dark: 0x141415)
    static let surface2 = Color(light: 0xF1F1EF, dark: 0x1C1C1E)
    static let ink = Color(light: 0x0A0A0A, dark: 0xF5F5F4)
    static let ink2 = Color(light: 0x3A3A3A, dark: 0xCFCFCF)
    static let muted = Color(light: 0x737373, dark: 0x8E8E8E)
    static let faint = Color(light: 0xA3A3A3, dark: 0x5C5C5C)
    static let line = Color(light: 0xE8E8E5, dark: 0x262628)
    static let lineStrong = Color(light: 0xD6D6D2, dark: 0x3A3A3C)
    static let ok = Color(light: 0x157F3B, dark: 0x3FB96B)
    static let okSoft = Color(light: 0x157F3B, dark: 0x3FB96B, alpha: (0.08, 0.12))
    static let bad = Color(light: 0xB42318, dark: 0xF06A5F)
    static let badSoft = Color(light: 0xB42318, dark: 0xF06A5F, alpha: (0.08, 0.12))
    static let warn = Color(light: 0x9A6700, dark: 0xE0A43A)
    static let warnSoft = Color(light: 0x9A6700, dark: 0xE0A43A, alpha: (0.10, 0.12))
    /// Over the part of the photo that stays private.
    static let overlay = Color(UIColor(rgb: 0x0A0A0A, alpha: 0.55))
    /// Text on an ink fill.
    static let onInk = Color(light: 0xFFFFFF, dark: 0x0A0A0A)

    /// Identifiers and hashes: 12 pt mono, on every surface.
    static let mono = Font.system(size: 12, design: .monospaced)

    enum Radius {
        static let small: CGFloat = 6
        static let control: CGFloat = 10
        static let card: CGFloat = 14
    }
}

enum Elevation { case flat, card, raised }

extension Theme {
    /// A card's border. Dark mode has no shadows, so the raised card takes the stronger line.
    static func border(_ level: Elevation) -> Color {
        level == .raised ? Color(light: 0xE8E8E5, dark: 0x3A3A3C) : line
    }
}

extension View {
    /// The two shadows of the system. Dark mode separates with lines only.
    func elevation(_ level: Elevation) -> some View { modifier(ElevationModifier(level: level)) }
}

private struct ElevationModifier: ViewModifier {
    let level: Elevation
    @Environment(\.colorScheme) private var scheme

    func body(content: Content) -> some View {
        let on = scheme == .light
        content
            .shadow(color: .black.opacity(on && level != .flat ? 0.05 : 0), radius: 1, y: 1)
            .shadow(color: .black.opacity(on && level == .raised ? 0.18 : 0), radius: 16, y: 12)
    }
}

extension Color {
    init(rgb: UInt32) {
        self.init(uiColor: UIColor(rgb: rgb))
    }

    init(light: UInt32, dark: UInt32, alpha: (CGFloat, CGFloat) = (1, 1)) {
        self.init(uiColor: UIColor { traits in
            traits.userInterfaceStyle == .dark ? UIColor(rgb: dark, alpha: alpha.1) : UIColor(rgb: light, alpha: alpha.0)
        })
    }
}

extension UIColor {
    convenience init(rgb: UInt32, alpha: CGFloat = 1) {
        self.init(
            red: CGFloat(rgb >> 16 & 0xff) / 255, green: CGFloat(rgb >> 8 & 0xff) / 255, blue: CGFloat(rgb & 0xff) / 255,
            alpha: alpha)
    }
}
