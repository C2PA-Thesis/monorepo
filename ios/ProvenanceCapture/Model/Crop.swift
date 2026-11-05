import Foundation

/// The size of the original every fingerprint and crop refers to: the whole
/// photo, scaled down when it has more pixels than the crop proof takes.
struct OriginalSize: Equatable, Hashable, Sendable {
    /// FINGERPRINT_MAX_PIXELS, and MAX_PIXELS in crates/fingerprint/src/image.rs.
    static let maxPixels = 524_288

    var width: Int
    var height: Int

    var pixels: Int { width * height }

    /// The size a `width` x `height` photo's original gets, as `Size::fit` in
    /// crates/fingerprint/src/image.rs computes it, with the same float operations.
    static func fit(width: Int, height: Int) -> OriginalSize {
        let product = width.multipliedReportingOverflow(by: height)
        if width > 0, height > 0, !product.overflow, product.partialValue <= maxPixels {
            return OriginalSize(width: width, height: height)
        }
        let scale = (Double(maxPixels) / (Double(width) * Double(height))).squareRoot()
        var fitted = OriginalSize(
            width: max(1, Int((Double(width) * scale).rounded(.down))),
            height: max(1, Int((Double(height) * scale).rounded(.down))))
        while fitted.pixels > maxPixels {
            if fitted.width >= fitted.height {
                fitted.width -= 1
            } else {
                fitted.height -= 1
            }
        }
        return fitted
    }
}

struct CropRect: Codable, Equatable, Hashable, Sendable {
    var x: Int
    var y: Int
    var width: Int
    var height: Int

    static func whole(_ size: OriginalSize) -> CropRect {
        CropRect(x: 0, y: 0, width: size.width, height: size.height)
    }

    func isWhole(in size: OriginalSize) -> Bool { self == .whole(size) }

    /// "512×512 at (0, 0)", as the web page and the CLI describe a crop.
    var text: String { "\(width)×\(height) at (\(x), \(y))" }
}

/// A handle on the crop box: four corners and four edges.
enum CropHandle: CaseIterable, Sendable {
    case topLeft, topRight, bottomLeft, bottomRight, top, bottom, left, right

    var isCorner: Bool { [.topLeft, .topRight, .bottomLeft, .bottomRight].contains(self) }
}

/// Moving and resizing the crop box, in original pixels. Every result fits
/// the original, which is what the crop proof accepts.
extension CropRect {
    /// Smallest side a handle can make.
    static let minSide = 64

    /// The same box, pulled back inside the original.
    func fitted(in size: OriginalSize) -> CropRect {
        let width = clamp(width, 1, size.width)
        let height = clamp(height, 1, size.height)
        return CropRect(
            x: clamp(x, 0, size.width - width), y: clamp(y, 0, size.height - height),
            width: width, height: height)
    }

    func moved(dx: Int, dy: Int, in size: OriginalSize) -> CropRect {
        CropRect(x: x + dx, y: y + dy, width: width, height: height).fitted(in: size)
    }

    /// Any shape: the handle moves its own edges.
    func resized(_ handle: CropHandle, dx: Int, dy: Int, in size: OriginalSize) -> CropRect {
        var left = x
        var top = y
        var right = x + width
        var bottom = y + height
        let min = Self.minSide
        if [.topLeft, .bottomLeft, .left].contains(handle) { left = clamp(left + dx, 0, right - min) }
        if [.topRight, .bottomRight, .right].contains(handle) { right = clamp(right + dx, left + min, size.width) }
        if [.topLeft, .topRight, .top].contains(handle) { top = clamp(top + dy, 0, bottom - min) }
        if [.bottomLeft, .bottomRight, .bottom].contains(handle) { bottom = clamp(bottom + dy, top + min, size.height) }
        return CropRect(x: left, y: top, width: right - left, height: bottom - top).fitted(in: size)
    }

    /// A fixed shape: the opposite corner stays put and the box keeps `ratio`.
    func resized(corner: CropHandle, dx: Int, dy: Int, ratio: Double, in size: OriginalSize) -> CropRect {
        let growsRight = corner == .topRight || corner == .bottomRight
        let growsDown = corner == .bottomLeft || corner == .bottomRight
        let anchorX = Double(growsRight ? x : x + width)
        let anchorY = Double(growsDown ? y : y + height)
        let byWidth = Double(width + (growsRight ? dx : -dx))
        let byHeight = Double(height + (growsDown ? dy : -dy)) * ratio
        let room = Swift.min(
            growsRight ? Double(size.width) - anchorX : anchorX,
            (growsDown ? Double(size.height) - anchorY : anchorY) * ratio)
        let newWidth = Int(clamp(Swift.max(byWidth, byHeight), Double(Self.minSide) * Swift.max(1, ratio), room).rounded(.down))
        let newHeight = Int((Double(newWidth) / ratio).rounded(.down))
        let anchorXi = Int(anchorX)
        let anchorYi = Int(anchorY)
        return CropRect(
            x: growsRight ? anchorXi : anchorXi - newWidth, y: growsDown ? anchorYi : anchorYi - newHeight,
            width: newWidth, height: newHeight).fitted(in: size)
    }

    /// The largest box of this shape that fits the original, centred where `current` is.
    static func fit(_ ratio: Double, around current: CropRect, in size: OriginalSize) -> CropRect {
        let width = Int(Swift.min(Double(size.width), Double(size.height) * ratio).rounded(.down))
        let height = Int((Double(width) / ratio).rounded(.down))
        let centerX = current.x + current.width / 2
        let centerY = current.y + current.height / 2
        return CropRect(x: centerX - width / 2, y: centerY - height / 2, width: width, height: height).fitted(in: size)
    }
}

func clamp<T: Comparable>(_ value: T, _ low: T, _ high: T) -> T {
    min(max(value, low), high)
}
