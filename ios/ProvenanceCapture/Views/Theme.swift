import SwiftUI
import UIKit

/// The tokens of the satellites PoC (poc-satellites-content-provenance), with
/// a dark mode of our own. Ink on paper; teal for what is in progress or
/// chosen, green for verified, red for what failed.
enum Theme {
    static let paper = Color(light: 0xFFFFFF, dark: 0x0B0B0C)
    static let surface = Color(light: 0xF6F6F3, dark: 0x151517)
    static let surface2 = Color(light: 0xEFEFEA, dark: 0x1D1D20)
    static let line = Color(light: 0xE7E7E1, dark: 0x27272B)
    static let line2 = Color(light: 0xDCDCD4, dark: 0x36363C)
    static let ink = Color(light: 0x0B0B0C, dark: 0xF4F4F1)
    static let ink2 = Color(light: 0x2A2A2E, dark: 0xD4D4D0)
    static let soft = Color(light: 0x6B6B73, dark: 0x9A9AA2)
    static let faint = Color(light: 0x9A9AA2, dark: 0x66666E)
    static let signal = Color(light: 0x0A7D72, dark: 0x2CC4B3)
    static let signalBright = Color(rgb: 0x14B8A6)
    static let signalSoft = Color(light: 0xE6F5F2, dark: 0x2CC4B3, alpha: (1, 0.14))
    static let verify = Color(light: 0x128A5C, dark: 0x34C486)
    static let verifySoft = Color(light: 0xE8F4EE, dark: 0x34C486, alpha: (1, 0.14))
    static let warn = Color(light: 0xB5762A, dark: 0xE0A04A)
    static let warnSoft = Color(light: 0xFBF1E4, dark: 0xE0A04A, alpha: (1, 0.14))
    static let danger = Color(light: 0xC2402F, dark: 0xEF6B5A)
    static let dangerSoft = Color(light: 0xFDECEB, dark: 0xEF6B5A, alpha: (1, 0.14))
    /// Text on an ink fill.
    static let onInk = Color(light: 0xFFFFFF, dark: 0x0B0B0C)
    /// Over the part of the photo that stays private.
    static let shade = Color.black.opacity(0.7)

    /// The proof seal's conic gradient.
    static let iris: [Color] = [0x7BE0D3, 0x4AA8FF, 0x8B7BFF, 0xD77BE0, 0xFFCF7B, 0x7BE0A3, 0x7BE0D3].map(Color.init(rgb:))

    enum Radius {
        static let xs: CGFloat = 8
        static let sm: CGFloat = 12
        static let md: CGFloat = 18
        static let lg: CGFloat = 26
    }

    static let ease = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.32)
}

/// Hanken Grotesk for text, IBM Plex Mono for identifiers. Sizes scale with Dynamic Type.
enum Typeface {
    enum Weight: String { case regular = "Regular", medium = "Medium", semibold = "SemiBold", bold = "Bold" }

    static func sans(_ size: CGFloat, _ weight: Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom("HankenGrotesk-\(weight.rawValue)", size: size, relativeTo: style)
    }

    static func mono(_ size: CGFloat = 12.5, medium: Bool = false) -> Font {
        .custom(medium ? "IBMPlexMono-Medium" : "IBMPlexMono-Regular", size: size, relativeTo: .caption)
    }

    static let display = sans(32, .bold, relativeTo: .largeTitle)
    static let title = sans(26, .semibold, relativeTo: .title)
    static let headline = sans(17, .semibold, relativeTo: .headline)
    static let body = sans(16, relativeTo: .body)
    static let callout = sans(15, .medium, relativeTo: .callout)
    static let footnote = sans(13.5, .medium, relativeTo: .footnote)
    static let caption = sans(12.5, .semibold, relativeTo: .caption)
}

extension View {
    /// The tracking the PoC gives display sizes.
    func tight(_ size: CGFloat) -> some View { tracking(-0.025 * size) }
}

// MARK: Buttons

/// The PoC's pill buttons: ink, outlined or quiet.
struct PillButtonStyle: ButtonStyle {
    enum Kind { case primary, ghost, quiet, danger, light }
    var kind = Kind.primary
    var busy = false
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Typeface.sans(16, .semibold, relativeTo: .headline))
            .lineLimit(1)
            .frame(maxWidth: .infinity, minHeight: kind == .quiet ? 44 : 54)
            .padding(.horizontal, 20)
            .foregroundStyle(foreground)
            .background(background, in: Capsule())
            .overlay(Capsule().strokeBorder(kind == .ghost ? Theme.line2 : .clear))
            .opacity(isEnabled || busy ? 1 : 0.45)
            .scaleEffect(configuration.isPressed ? 0.985 : 1)
            .offset(y: configuration.isPressed ? 1 : 0)
            .animation(.easeOut(duration: 0.16), value: configuration.isPressed)
            .contentShape(Capsule())
    }

    private var foreground: Color {
        switch kind {
        case .primary: Theme.onInk
        case .ghost, .quiet: Theme.ink
        case .danger: .white
        case .light: .black
        }
    }

    private var background: Color {
        switch kind {
        case .primary: Theme.ink
        case .ghost: Theme.paper
        case .quiet: .clear
        case .danger: Theme.danger
        case .light: .white
        }
    }
}

extension ButtonStyle where Self == PillButtonStyle {
    static var primary: PillButtonStyle { PillButtonStyle(kind: .primary) }
    static func primary(busy: Bool) -> PillButtonStyle { PillButtonStyle(kind: .primary, busy: busy) }
    static var ghost: PillButtonStyle { PillButtonStyle(kind: .ghost) }
    static var quiet: PillButtonStyle { PillButtonStyle(kind: .quiet) }
    static var light: PillButtonStyle { PillButtonStyle(kind: .light) }
}

// MARK: Color helpers

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
