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

    func contains(_ point: CropPoint) -> Bool {
        point.x >= x && point.x <= x + width && point.y >= y && point.y <= y + height
    }

    static let whole = CropRect(x: 0, y: 0, width: Original.width, height: Original.height)

    var isWhole: Bool { self == .whole }

    /// "512×512 at (0, 0)", as the web page and the CLI describe a crop.
    var text: String { "\(width)×\(height) at (\(x), \(y))" }
}

struct CropPoint: Equatable, Sendable {
    var x: Int
    var y: Int
}

struct CropPreset: Identifiable, Sendable {
    let name: String
    let rect: CropRect
    var id: String { name }

    static let leftHalf = CropRect(x: 0, y: 0, width: 512, height: 512)

    /// The same presets as web/src/crop.ts.
    static let all: [CropPreset] = [
        CropPreset(name: "Left half", rect: leftHalf),
        CropPreset(name: "Center", rect: CropRect(x: 256, y: 0, width: 512, height: 512)),
        CropPreset(name: "Right half", rect: CropRect(x: 512, y: 0, width: 512, height: 512)),
        CropPreset(name: "Whole frame", rect: CropRect(x: 0, y: 0, width: Original.width, height: Original.height)),
    ]
}

/// One drag over the frame, in original pixel coordinates: a drag that starts
/// inside the box moves it, anywhere else it draws a new one (web/src/crop.ts).
struct CropDrag: Sendable {
    static let minSide = 16

    let start: CropPoint
    let moving: CropRect?

    init(start: CropPoint, current: CropRect) {
        self.start = start
        moving = current.contains(start) ? current : nil
    }

    func rect(to end: CropPoint) -> CropRect {
        let width = Original.width
        let height = Original.height
        if let from = moving {
            return CropRect(
                x: clamp(from.x + end.x - start.x, 0, width - from.width),
                y: clamp(from.y + end.y - start.y, 0, height - from.height),
                width: from.width, height: from.height)
        }
        let x = min(start.x, end.x)
        let y = min(start.y, end.y)
        let left = min(x, width - Self.minSide)
        let top = min(y, height - Self.minSide)
        return CropRect(
            x: left, y: top,
            width: clamp(abs(end.x - start.x), Self.minSide, width - left),
            height: clamp(abs(end.y - start.y), Self.minSide, height - top))
    }

    /// A point on a view `viewWidth` points wide that shows the whole original.
    static func originalPoint(x: Double, y: Double, viewWidth: Double) -> CropPoint {
        let scale = Double(Original.width) / viewWidth
        return CropPoint(
            x: clamp(Int((x * scale).rounded()), 0, Original.width),
            y: clamp(Int((y * scale).rounded()), 0, Original.height))
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
