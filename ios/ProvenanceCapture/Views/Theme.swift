import SwiftUI
import UIKit

/// The palette of web/src/style.css, light and dark.
enum Theme {
    static let bg = Color(light: 0xf3f4f7, dark: 0x0e1116)
    static let surface = Color(light: 0xffffff, dark: 0x171b22)
    static let surface2 = Color(light: 0xf6f7f9, dark: 0x1d222b)
    static let ink = Color(light: 0x161b24, dark: 0xe8ebf0)
    static let muted = Color(light: 0x5d6676, dark: 0xa1a9b6)
    static let faint = Color(light: 0x8a93a3, dark: 0x6f7886)
    static let line = Color(light: 0xe1e4ea, dark: 0x2a303b)
    static let accent = Color(light: 0x2f6fde, dark: 0x5b8ff0)
    static let accentSoft = Color(light: 0xe7effc, dark: 0x1b2a45)
    static let ok = Color(light: 0x1f8a5b, dark: 0x3cc488)
    static let okSoft = Color(light: 0xe3f4ec, dark: 0x14301f)
    static let bad = Color(light: 0xc62f3c, dark: 0xf0606b)
    static let badSoft = Color(light: 0xfbe7e9, dark: 0x3a1a1e)
    static let privateInk = Color(light: 0xa15c07, dark: 0xe6a545)
    static let privateSoft = Color(light: 0xfbf0dc, dark: 0x33260f)
    static let publicInk = Color(light: 0x2f6fde, dark: 0x6b9cf3)
    static let publicSoft = Color(light: 0xe7effc, dark: 0x1b2a45)
    static let you = Color(rgb: 0xe8590c)
}

extension Color {
    init(rgb: UInt32) {
        self.init(uiColor: UIColor(rgb: rgb))
    }

    init(light: UInt32, dark: UInt32) {
        self.init(uiColor: UIColor { traits in UIColor(rgb: traits.userInterfaceStyle == .dark ? dark : light) })
    }
}

extension UIColor {
    convenience init(rgb: UInt32) {
        self.init(
            red: CGFloat(rgb >> 16 & 0xff) / 255, green: CGFloat(rgb >> 8 & 0xff) / 255, blue: CGFloat(rgb & 0xff) / 255,
            alpha: 1)
    }
}
