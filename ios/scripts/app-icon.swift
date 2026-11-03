import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// Draws the viewfinder app icon at 1024 px, light and dark.
// Run: swift ios/scripts/app-icon.swift ios/ProvenanceCapture/Resources/Assets.xcassets/AppIcon.appiconset
func draw(_ out: String, paper: UInt32, ink: UInt32, signal: UInt32) {
    let side = 1024
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    func color(_ hex: UInt32) -> CGColor {
        CGColor(srgbRed: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
    ctx.setFillColor(color(paper))
    ctx.fill(CGRect(x: 0, y: 0, width: side, height: side))
    // Flip to a top-left origin and fit the 100 box at 80% so the corners clear the icon mask.
    ctx.translateBy(x: 0, y: CGFloat(side))
    ctx.scaleBy(x: 1, y: -1)
    let scale = CGFloat(side) / 100 * 0.8
    ctx.translateBy(x: CGFloat(side) * 0.1, y: CGFloat(side) * 0.1)
    ctx.scaleBy(x: scale, y: scale)
    let brackets = CGMutablePath()
    for (x, y, dx, dy) in [(14.0, 14.0, 1.0, 1.0), (86, 14, -1, 1), (86, 86, -1, -1), (14, 86, 1, -1)] {
        brackets.move(to: CGPoint(x: x, y: y + 18 * dy))
        brackets.addArc(tangent1End: CGPoint(x: x, y: y), tangent2End: CGPoint(x: x + 18 * dx, y: y), radius: 6)
        brackets.addLine(to: CGPoint(x: x + 18 * dx, y: y))
    }
    ctx.addPath(brackets)
    ctx.setStrokeColor(color(ink))
    ctx.setLineWidth(8.5)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.strokePath()
    let hexagon = CGMutablePath()
    hexagon.addLines(between: [(50, 30), (67.3, 40), (67.3, 60), (50, 70), (32.7, 60), (32.7, 40)].map { CGPoint(x: $0.0, y: $0.1) })
    hexagon.closeSubpath()
    ctx.addPath(hexagon)
    ctx.setFillColor(color(signal))
    ctx.setStrokeColor(color(signal))
    ctx.setLineWidth(5)
    ctx.drawPath(using: .fillStroke)
    let image = ctx.makeImage()!
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    CGImageDestinationFinalize(dest)
}

let dir = CommandLine.arguments[1]
draw("\(dir)/icon-1024.png", paper: 0xFFFFFF, ink: 0x0B0B0C, signal: 0x0A7D72)
draw("\(dir)/icon-1024-dark.png", paper: 0x0B0B0C, ink: 0xF4F4F1, signal: 0x2CC4B3)
