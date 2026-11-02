import Foundation

/// The 1024x512 original every fingerprint and crop refers to.
enum Original {
    static let width = 1024
    static let height = 512
    static let pixels = width * height
}

struct CropRect: Codable, Equatable, Hashable, Sendable {
    var x: Int
    var y: Int
    var width: Int
    var height: Int

    static let whole = CropRect(x: 0, y: 0, width: Original.width, height: Original.height)

    var isWhole: Bool { self == .whole }

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
    var fitted: CropRect {
        let width = clamp(width, 1, Original.width)
        let height = clamp(height, 1, Original.height)
        return CropRect(
            x: clamp(x, 0, Original.width - width), y: clamp(y, 0, Original.height - height),
            width: width, height: height)
    }

    func moved(dx: Int, dy: Int) -> CropRect {
        CropRect(x: x + dx, y: y + dy, width: width, height: height).fitted
    }

    /// Any shape: the handle moves its own edges.
    func resized(_ handle: CropHandle, dx: Int, dy: Int) -> CropRect {
        var left = x
        var top = y
        var right = x + width
        var bottom = y + height
        let min = Self.minSide
        if [.topLeft, .bottomLeft, .left].contains(handle) { left = clamp(left + dx, 0, right - min) }
        if [.topRight, .bottomRight, .right].contains(handle) { right = clamp(right + dx, left + min, Original.width) }
        if [.topLeft, .topRight, .top].contains(handle) { top = clamp(top + dy, 0, bottom - min) }
        if [.bottomLeft, .bottomRight, .bottom].contains(handle) { bottom = clamp(bottom + dy, top + min, Original.height) }
        return CropRect(x: left, y: top, width: right - left, height: bottom - top).fitted
    }

    /// A fixed shape: the opposite corner stays put and the box keeps `ratio`.
    func resized(corner: CropHandle, dx: Int, dy: Int, ratio: Double) -> CropRect {
        let growsRight = corner == .topRight || corner == .bottomRight
        let growsDown = corner == .bottomLeft || corner == .bottomRight
        let anchorX = Double(growsRight ? x : x + width)
        let anchorY = Double(growsDown ? y : y + height)
        let byWidth = Double(width + (growsRight ? dx : -dx))
        let byHeight = Double(height + (growsDown ? dy : -dy)) * ratio
        let room = Swift.min(
            growsRight ? Double(Original.width) - anchorX : anchorX,
            (growsDown ? Double(Original.height) - anchorY : anchorY) * ratio)
        let newWidth = Int(clamp(Swift.max(byWidth, byHeight), Double(Self.minSide) * Swift.max(1, ratio), room).rounded(.down))
        let newHeight = Int((Double(newWidth) / ratio).rounded(.down))
        let anchorXi = Int(anchorX)
        let anchorYi = Int(anchorY)
        return CropRect(
            x: growsRight ? anchorXi : anchorXi - newWidth, y: growsDown ? anchorYi : anchorYi - newHeight,
            width: newWidth, height: newHeight).fitted
    }

    /// The largest box of this shape that fits the original, centred where `current` is.
    static func fit(_ ratio: Double, around current: CropRect) -> CropRect {
        let width = Int(Swift.min(Double(Original.width), Double(Original.height) * ratio).rounded(.down))
        let height = Int((Double(width) / ratio).rounded(.down))
        let centerX = current.x + current.width / 2
        let centerY = current.y + current.height / 2
        return CropRect(x: centerX - width / 2, y: centerY - height / 2, width: width, height: height).fitted
    }
}

/// The 2:1 window the photographer slides over a photo of another shape.
struct FramingWindow: Equatable, Sendable {
    let photoWidth: Double
    let photoHeight: Double
    /// Whether the photo is at least 2:1, so the window slides horizontally.
    let wide: Bool
    let width: Double
    let height: Double

    init(photoWidth: Double, photoHeight: Double) {
        self.photoWidth = photoWidth
        self.photoHeight = photoHeight
        let ratio = Double(Original.width) / Double(Original.height)
        wide = photoWidth / photoHeight >= ratio
        width = wide ? photoHeight * ratio : photoWidth
        height = wide ? photoHeight : photoWidth / ratio
    }

    /// Whether there is more than a pixel of photo to slide over.
    var slidable: Bool { wide ? photoWidth - width > 1 : photoHeight - height > 1 }

    /// The window's top-left corner in photo pixels, `position` 0 to 1 along the longer axis.
    func origin(at position: Double) -> (x: Double, y: Double) {
        let position = min(max(position, 0), 1)
        return wide ? ((photoWidth - width) * position, 0) : (0, (photoHeight - height) * position)
    }
}

func clamp<T: Comparable>(_ value: T, _ low: T, _ high: T) -> T {
    min(max(value, low), high)
}
